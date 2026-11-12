use senate_core::engine::{ChatSend, TransitionArgs};
use senate_core::model::*;
use senate_core::store::Store;
use senate_core::tools::{self, ToolCtx};
use serde_json::json;

fn setup() -> (Store, Agent, Agent, Project) {
    let st = Store::open_in_memory().unwrap();
    let dev = st.create_agent(NewAgent { name: "Разработчик".into(), role: "developer".into(), ..Default::default() }).unwrap();
    let arch = st.create_agent(NewAgent { name: "Архитектор".into(), role: "architect".into(), ..Default::default() }).unwrap();
    let p = st
        .create_project(NewProject { name: "Сайт".into(), goal: "Запустить".into(), members: vec![dev.id.clone(), arch.id.clone()], ..Default::default() })
        .unwrap();
    (st, dev, arch, p)
}

fn obl(st: &Store, owner: &Agent, p: &Project, criteria: &str) -> Obligation {
    st.create_obligation(NewObligation {
        title: "Сделать форму".into(),
        result: "Форма работает".into(),
        criteria: criteria.into(),
        owner_id: Some(owner.id.clone()),
        project_id: Some(p.id.clone()),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn done_requires_a_delivered_result() {
    let (st, dev, _, p) = setup();
    let o = obl(&st, &dev, &p, "");
    st.transition(&o.id, ObligationState::Running, Default::default(), "system").unwrap();
    let err = st.transition(&o.id, ObligationState::Done, Default::default(), &dev.id).unwrap_err();
    assert!(err.to_string().contains("без результата"), "{err}");
    let ok = st
        .transition(&o.id, ObligationState::Done, TransitionArgs { outcome: Some("форма в ветке feature/form".into()), ..Default::default() }, &dev.id)
        .unwrap();
    assert_eq!(ok.state, ObligationState::Done);
}

#[test]
fn waiting_blocked_cancelled_need_their_reasons() {
    let (st, dev, _, p) = setup();
    let o = obl(&st, &dev, &p, "");
    assert!(st.transition(&o.id, ObligationState::Waiting, Default::default(), "x").is_err());
    assert!(st.transition(&o.id, ObligationState::Blocked, Default::default(), "x").is_err());
    assert!(st.transition(&o.id, ObligationState::Cancelled, Default::default(), "x").is_err());
    st.transition(&o.id, ObligationState::Blocked, TransitionArgs { blocker: Some("нужен доступ".into()), ..Default::default() }, "x").unwrap();
    // terminal states stay terminal
    st.transition(&o.id, ObligationState::Cancelled, TransitionArgs { outcome: Some("не нужно".into()), ..Default::default() }, "user").unwrap();
    assert!(st.transition(&o.id, ObligationState::Accepted, Default::default(), "x").is_err());
}

#[test]
fn cannot_finish_while_delegated_work_is_open_and_cancel_cascades() {
    let (st, dev, arch, p) = setup();
    let parent = obl(&st, &dev, &p, "");
    let child = st
        .create_obligation(NewObligation { title: "Схема".into(), owner_id: Some(arch.id.clone()), project_id: Some(p.id.clone()), parent_id: Some(parent.id.clone()), ..Default::default() })
        .unwrap();
    st.transition(&parent.id, ObligationState::Running, Default::default(), "x").unwrap();
    let e = st.transition(&parent.id, ObligationState::Done, TransitionArgs { outcome: Some("ок".into()), ..Default::default() }, "x");
    assert!(e.is_err());
    st.transition(&parent.id, ObligationState::Cancelled, TransitionArgs { outcome: Some("отмена".into()), ..Default::default() }, "user").unwrap();
    assert_eq!(st.get_obligation(&child.id).unwrap().state, ObligationState::Cancelled);
}

#[test]
fn circular_waits_are_rejected() {
    let (st, dev, arch, p) = setup();
    let a = obl(&st, &dev, &p, "");
    let b = obl(&st, &arch, &p, "");
    st.transition(&a.id, ObligationState::Waiting, TransitionArgs { wait: Some(WaitCondition::Obligation { id: b.id.clone() }), ..Default::default() }, "x").unwrap();
    let e = st.transition(&b.id, ObligationState::Waiting, TransitionArgs { wait: Some(WaitCondition::Obligation { id: a.id.clone() }), ..Default::default() }, "x");
    assert!(e.unwrap_err().to_string().contains("круговая"));
}

#[test]
fn waiting_obligation_wakes_on_time_and_on_dependency() {
    let (st, dev, arch, p) = setup();
    let a = obl(&st, &dev, &p, "");
    st.transition(&a.id, ObligationState::Waiting, TransitionArgs { wait: Some(WaitCondition::Time { at: 1 }), ..Default::default() }, "x").unwrap();
    let woken = st.wake_due().unwrap();
    assert_eq!(woken, vec![a.id.clone()]);
    assert_eq!(st.get_obligation(&a.id).unwrap().state, ObligationState::Accepted);

    let dep = obl(&st, &arch, &p, "");
    let b = obl(&st, &dev, &p, "");
    st.transition(&b.id, ObligationState::Waiting, TransitionArgs { wait: Some(WaitCondition::Obligation { id: dep.id.clone() }), ..Default::default() }, "x").unwrap();
    assert!(st.wake_due().unwrap().is_empty());
    st.transition(&dep.id, ObligationState::Done, TransitionArgs { outcome: Some("готово".into()), ..Default::default() }, "x").unwrap();
    assert_eq!(st.wake_due().unwrap(), vec![b.id.clone()]);
}

#[test]
fn scheduler_never_creates_two_live_sessions_for_one_obligation() {
    let (st, dev, _, p) = setup();
    let o = obl(&st, &dev, &p, "");
    let first = st.schedule("mock").unwrap();
    assert_eq!(first.sessions.len(), 1);
    let second = st.schedule("mock").unwrap();
    assert!(second.sessions.is_empty(), "duplicate event must not duplicate work");
    // claim → running
    let s = st.claim_session("node", &|_| true).unwrap().unwrap();
    st.on_session_started(&s).unwrap();
    assert_eq!(st.get_obligation(&o.id).unwrap().state, ObligationState::Running);
    // a crash leaves the session running; restart requeues it, not duplicates it
    assert_eq!(st.requeue_orphaned_sessions().unwrap(), 1);
    assert_eq!(st.list_sessions(None, Some(&o.id), Some(SessionStatus::Queued), 10).unwrap().len(), 1);
}

#[test]
fn memory_lifecycle_corrections_and_promotion() {
    let (st, dev, _, p) = setup();
    let m = st
        .add_memory(NewMemory {
            level: MemoryLevel::Project, company_id: None,
            agent_id: Some(dev.id.clone()),
            project_id: Some(p.id.clone()),
            kind: MemoryKind::Observation,
            body: "API отдаёт даты в UTC".into(),
            source: "сессия 1".into(),
            scope_note: String::new(),
            verification: String::new(),
            fresh_until: None,
        })
        .unwrap();
    // verified needs a basis
    assert!(st.verify_memory(&m.id, "", "x").is_err());
    // not verified -> cannot be promoted to collective
    assert!(st.promote_memory(&m.id, "x").is_err());
    st.verify_memory(&m.id, "воспроизведено запросом к /time", "x").unwrap();
    let hits = st.search_memory("даты UTC", Some(&dev.id), Some(&p.id), 5).unwrap();
    assert_eq!(hits.len(), 1);

    // correction: old one stops being used, new one is a hypothesis again
    let newer = st.supersede_memory(&m.id, "API отдаёт даты в локальной зоне сервера", "сессия 2", "x").unwrap();
    assert_eq!(newer.kind, MemoryKind::Hypothesis);
    let hits = st.search_memory("даты", Some(&dev.id), Some(&p.id), 5).unwrap();
    assert!(hits.iter().all(|h| h.id != m.id), "superseded entry must not be returned");
    assert!(hits.iter().any(|h| h.id == newer.id));
    assert_eq!(st.get_memory(&m.id).unwrap().status, MemoryStatus::Superseded);
}

#[test]
fn memory_visibility_follows_scope() {
    let (st, dev, arch, p) = setup();
    st.add_memory(NewMemory { level: MemoryLevel::Personal, company_id: None, agent_id: Some(dev.id.clone()), project_id: None, kind: MemoryKind::Preference, body: "люблю короткие коммиты".into(), source: "user".into(), scope_note: "".into(), verification: "".into(), fresh_until: None }).unwrap();
    assert_eq!(st.search_memory("коммиты", Some(&dev.id), Some(&p.id), 5).unwrap().len(), 1);
    assert!(st.search_memory("коммиты", Some(&arch.id), Some(&p.id), 5).unwrap().is_empty(), "personal memory is private to its owner");
}

#[test]
fn chat_routes_by_mention_and_topic_and_resumes_context() {
    let (st, dev, arch, p) = setup();
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), ..Default::default() }).unwrap();
    let d = st.send_chat(ChatSend { thread_id: t.id.clone(), text: format!("@{} как разложить модули?", arch.slug), ..Default::default() }, "mock").unwrap();
    assert_eq!(d.sessions.len(), 1);
    assert_eq!(d.sessions[0].agent_id, arch.id);
    assert_eq!(d.agent_messages[0].status, MessageStatus::Queued);

    let t2 = st.create_thread(NewThread { project_id: Some(p.id.clone()), ..Default::default() }).unwrap();
    let d2 = st.send_chat(ChatSend { thread_id: t2.id.clone(), text: "Починить баг в тесте".into(), ..Default::default() }, "mock").unwrap();
    assert_eq!(d2.sessions[0].agent_id, dev.id);
    // title comes from the first message
    assert!(st.get_thread(&t2.id).unwrap().title.starts_with("Починить баг"));
    // the CLI session id is remembered per chat and backend
    st.set_thread_cli_session(&t2.id, "claude", "abc").unwrap();
    assert_eq!(st.thread_cli_session(&t2.id, "claude").unwrap().as_deref(), Some("abc"));
}

#[test]
fn tools_are_authorized_by_the_environment() {
    let (st, dev, arch, p) = setup();
    let ctx = ToolCtx { agent_id: dev.id.clone(), project_id: Some(p.id.clone()), ..Default::default() };

    // remembering is allowed by default
    let r = tools::call(&st, &ctx, "senate_remember", &json!({"body": "тесты запускать через cargo test", "kind": "observation"})).unwrap();
    assert_eq!(r["status"], "saved");
    // verified without a basis is refused
    assert!(tools::call(&st, &ctx, "senate_remember", &json!({"body": "x", "kind": "verified"})).is_err());

    // a deny grant beats the default
    st.set_grant(Some(&dev.id), None, action::OBLIGATION_CREATE, GrantMode::Deny).unwrap();
    let e = tools::call(&st, &ctx, "senate_obligation_create", &json!({"title": "t", "result": "r"})).unwrap_err();
    assert!(matches!(e, senate_core::Error::Forbidden(_)));
    st.set_grant(Some(&dev.id), None, action::OBLIGATION_CREATE, GrantMode::Auto).unwrap();

    // delegation of work to a colleague with approval required becomes a pending approval
    st.set_grant(None, None, action::OBLIGATION_DELEGATE, GrantMode::Approve).unwrap();
    let r = tools::call(&st, &ctx, "senate_obligation_create", &json!({"title": "Схема БД", "result": "ER-диаграмма", "owner": arch.slug})).unwrap();
    assert_eq!(r["status"], "pending_approval");
    assert!(st.list_obligations(Some(&arch.id), None, true).unwrap().is_empty());
    let ap = st.list_approvals(true).unwrap().remove(0);
    st.decide_approval(&ap.id, true).unwrap();
    assert_eq!(st.list_obligations(Some(&arch.id), None, true).unwrap().len(), 1, "approved action executes exactly once");
    assert!(st.decide_approval(&ap.id, true).is_err(), "an approval cannot be replayed");
}

#[test]
fn agent_cannot_touch_foreign_obligations_and_done_with_criteria_goes_to_review() {
    let (st, dev, arch, p) = setup();
    let mine = obl(&st, &dev, &p, "тест зелёный");
    let ctx_dev = ToolCtx { agent_id: dev.id.clone(), obligation_id: Some(mine.id.clone()), project_id: Some(p.id.clone()), ..Default::default() };
    let ctx_arch = ToolCtx { agent_id: arch.id.clone(), project_id: Some(p.id.clone()), ..Default::default() };
    st.transition(&mine.id, ObligationState::Running, Default::default(), "x").unwrap();
    let r = tools::call(&st, &ctx_arch, "senate_obligation_update", &json!({"id": mine.id, "state": "done", "outcome": "x"})).unwrap();
    assert_eq!(r["status"], "pending_approval", "someone else's task waits for the person");
    assert_eq!(st.get_obligation(&mine.id).unwrap().state, ObligationState::Running, "and nothing changes meanwhile");
    let r = tools::call(&st, &ctx_dev, "senate_obligation_update", &json!({"state": "done", "outcome": "реализовано, тесты зелёные"})).unwrap();
    assert_eq!(r["state"], "review", "an author's claim of done is checked against criteria first");
    // the user accepts it
    st.transition(&mine.id, ObligationState::Done, Default::default(), "user").unwrap();
}

#[test]
fn retiring_requires_handing_over_open_work_and_profile_changes_are_versioned() {
    let (st, dev, _, p) = setup();
    obl(&st, &dev, &p, "");
    assert!(st.retire_agent(&dev.id).is_err());
    st.update_profile(&dev.id, "новый профиль", "уточнили оптику", "user").unwrap();
    assert_eq!(st.profile_versions(&dev.id).unwrap().len(), 2);
}

#[test]
fn project_requirement_change_is_journaled_and_decisions_supersede() {
    let (st, _, _, p) = setup();
    st.update_project(&p.id, senate_core::store::ProjectPatch { goal: Some("Новая цель".into()), ..Default::default() }).unwrap();
    assert!(st.events(Some("project"), Some(&p.id), 10).unwrap().iter().any(|e| e.kind == "project.requirements_changed"));
    let d1 = st.propose_decision(&p.id, "Используем SQLite", "", "user", None).unwrap();
    st.set_decision_status(&d1.id, DecisionStatus::Accepted, "user").unwrap();
    let d2 = st.propose_decision(&p.id, "Переходим на Postgres", "", "user", Some(&d1.id)).unwrap();
    st.set_decision_status(&d2.id, DecisionStatus::Accepted, "user").unwrap();
    let all = st.list_decisions(&p.id).unwrap();
    assert_eq!(all.iter().find(|d| d.id == d1.id).unwrap().status, DecisionStatus::Superseded);
}


#[test]
fn a_specialist_keeps_separate_memory_per_company_and_shares_company_decisions() {
    let st = Store::open_in_memory().unwrap();
    let designer = st.create_agent(NewAgent { name: "Дизайнер".into(), role: "designer".into(), ..Default::default() }).unwrap();
    let acme = st.create_company(NewCompany { name: "Acme".into(), brief: "Строгий минимализм".into(), color: String::new() }).unwrap();
    let globex = st.create_company(NewCompany { name: "Globex".into(), ..Default::default() }).unwrap();
    let p_acme = st.create_project(NewProject { name: "Сайт Acme".into(), company_id: Some(acme.id.clone()), ..Default::default() }).unwrap();
    let p_acme2 = st.create_project(NewProject { name: "Приложение Acme".into(), company_id: Some(acme.id.clone()), ..Default::default() }).unwrap();
    let p_globex = st.create_project(NewProject { name: "Лендинг Globex".into(), company_id: Some(globex.id.clone()), ..Default::default() }).unwrap();

    let ctx = |p: &Project, co: &Company| ToolCtx { agent_id: designer.id.clone(), project_id: Some(p.id.clone()), company_id: Some(co.id.clone()), ..Default::default() };
    // craft that follows the designer everywhere
    tools::call(&st, &ctx(&p_acme, &acme), "senate_remember", &json!({"body": "сетка на 8px даёт ритм интерфейса", "kind": "observation", "level": "personal"})).unwrap();
    // what the designer knows about Acme only (private) and what everyone at Acme should know
    tools::call(&st, &ctx(&p_acme, &acme), "senate_remember", &json!({"body": "у Acme фирменный цвет синий, скругления запрещены", "kind": "observation", "level": "company_personal"})).unwrap();
    tools::call(&st, &ctx(&p_acme, &acme), "senate_remember", &json!({"body": "Acme согласует макеты только по четвергам", "kind": "observation", "level": "company"})).unwrap();
    tools::call(&st, &ctx(&p_globex, &globex), "senate_remember", &json!({"body": "у Globex фирменный цвет оранжевый", "kind": "observation", "level": "company_personal"})).unwrap();

    let seen = |p: &Project| -> Vec<String> { st.search_memory_in("цвет сетка согласует", Some(&designer.id), Some(&p.id), None, 20).unwrap().into_iter().map(|m| m.body).collect() };
    let at_acme = seen(&p_acme);
    assert!(at_acme.iter().any(|b| b.contains("сетка на 8px")), "general craft follows the specialist");
    assert!(at_acme.iter().any(|b| b.contains("Acme фирменный")), "own experience of this company");
    assert!(at_acme.iter().any(|b| b.contains("по четвергам")), "shared company knowledge");
    assert!(!at_acme.iter().any(|b| b.contains("Globex")), "another company's experience must not leak");
    // a second project of the same company sees the same company knowledge
    assert!(seen(&p_acme2).iter().any(|b| b.contains("по четвергам")));
    // and at Globex the Acme things are invisible
    let at_globex = seen(&p_globex);
    assert!(at_globex.iter().any(|b| b.contains("Globex")) && !at_globex.iter().any(|b| b.contains("Acme")));

    // a company-wide decision reaches every project of the company
    let d = tools::call(&st, &ctx(&p_acme, &acme), "senate_decision", &json!({"title": "Шрифт — только Inter", "scope": "company"})).unwrap();
    st.set_decision_status(d["id"].as_str().unwrap(), DecisionStatus::Accepted, "user").unwrap();
    let prompt = senate_core::workset::system_prompt(&st, &designer, Some(&p_acme2), None).unwrap();
    assert!(prompt.contains("Шрифт — только Inter") && prompt.contains("Строгий минимализм"), "{prompt}");
    let prompt = senate_core::workset::system_prompt(&st, &designer, Some(&p_globex), None).unwrap();
    assert!(!prompt.contains("Шрифт — только Inter"));
}


#[test]
fn naming_a_company_is_enough_the_environment_finds_it_attaches_the_chat_and_brings_its_memory() {
    let st = Store::open_in_memory().unwrap();
    let designer = st.create_agent(NewAgent { name: "Дизайнер".into(), role: "designer".into(), ..Default::default() }).unwrap();
    let co = st.create_company(NewCompany { name: "Вокруг света".into(), brief: "Туристическая компания".into(), color: String::new() }).unwrap();
    let other = st.create_company(NewCompany { name: "Ассет".into(), ..Default::default() }).unwrap();
    let p = st.create_project(NewProject { name: "Вокруг света".into(), company_id: Some(co.id.clone()), ..Default::default() }).unwrap();
    st.create_project(NewProject { name: "Общее".into(), ..Default::default() }).unwrap();
    st.add_memory(NewMemory { level: MemoryLevel::Project, company_id: None, agent_id: None, project_id: Some(p.id.clone()), kind: MemoryKind::Observation, body: "Дизайн-система Вокруг света: основной цвет тёмно-синий, шрифт Inter, скругления 12px".into(), source: "mbox".into(), scope_note: String::new(), verification: String::new(), fresh_until: None }).unwrap();
    st.add_memory(NewMemory { level: MemoryLevel::Company, company_id: Some(other.id.clone()), agent_id: None, project_id: None, kind: MemoryKind::Observation, body: "Дизайн-система Ассета: оранжевый".into(), source: "mbox".into(), scope_note: String::new(), verification: String::new(), fresh_until: None }).unwrap();

    // declensions and word order do not matter; generic names never match
    assert_eq!(st.mentions("Что известно о дизайн системе вокруг света?").unwrap().len(), 2, "the company and the project of that name");
    assert!(st.mentions("это общее правило").unwrap().is_empty(), "generic names are never matched");

    // an unattached chat: the person only says the name
    let t = st.create_thread(NewThread::default()).unwrap();
    let d = st.send_chat(ChatSend { thread_id: t.id.clone(), text: "Что можешь сказать о дизайн системе вокруг света?".into(), ..Default::default() }, "mock").unwrap();
    let t = st.get_thread(&t.id).unwrap();
    assert_eq!(t.project_id.as_deref(), Some(p.id.as_str()), "attached to the project, and through it to the company");
    assert_eq!(t.company_id.as_deref(), Some(co.id.as_str()));
    // what the designer receives: the memory of that company, not of the other one
    let prompt = &d.sessions[0].prompt;
    let system = senate_core::workset::system_prompt(&st, &designer, Some(&st.get_project(&p.id).unwrap()), t.company_id.as_deref()).unwrap();
    assert!(system.contains("Туристическая компания"));
    assert!(prompt.contains("тёмно-синий") || system.contains("тёмно-синий") || {
        let mem = st.search_memory_in("дизайн система", Some(&designer.id), Some(&p.id), t.company_id.as_deref(), 8).unwrap();
        mem.iter().any(|m| m.body.contains("тёмно-синий"))
    });
    assert!(!prompt.contains("оранжевый"), "another company's knowledge must not appear");

    // two different companies named: ambiguous, so nothing is attached by guesswork
    let t2 = st.create_thread(NewThread::default()).unwrap();
    st.send_chat(ChatSend { thread_id: t2.id.clone(), text: "Сравни ассет и вокруг света".into(), ..Default::default() }, "mock").unwrap();
    assert!(st.get_thread(&t2.id).unwrap().company_id.is_none());
}

#[test]
fn preview_links_are_local_only() {
    use senate_core::tools::local_preview_url;
    let ok = |u: &str| local_preview_url(u).map(|x| x.0);
    assert_eq!(ok("http://localhost:3000"), Some("http://localhost:3000".into()));
    assert_eq!(ok(" http://127.0.0.1:5173/cruises?x=1#top "), Some("http://127.0.0.1:5173/cruises?x=1#top".into()));
    assert_eq!(ok("https://[::1]:8443/a"), Some("https://[::1]:8443/a".into()));
    assert_eq!(ok("http://app.localhost:3000"), Some("http://app.localhost:3000".into()));
    assert_eq!(local_preview_url("http://localhost:3000").unwrap().1, "localhost:3000");
    for bad in [
        "http://example.com", "https://localhost.evil.com", "http://localhost@evil.com", "http://evil.com#@localhost",
        "http://localhost:99999", "http://localhost:abc", "http://localhost:", "ftp://localhost", "javascript:alert(1)",
        "file:///C:/x", "http://192.168.1.5:3000", "http://0.0.0.0:3000", "//localhost:3000", r"http://localhost:3000\@evil.com",
        "http://local host", "", "localhost:3000",
    ] {
        assert!(local_preview_url(bad).is_none(), "{bad}");
    }
}

#[test]
fn the_agent_controls_the_preview_button_of_its_chat() {
    let (st, dev, _, p) = setup();
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), agent_id: Some(dev.id.clone()), ..Default::default() }).unwrap();
    let ctx = ToolCtx { agent_id: dev.id.clone(), thread_id: Some(t.id.clone()), project_id: Some(p.id.clone()), ..Default::default() };

    let r = tools::call(&st, &ctx, "senate_preview", &json!({"url": "http://localhost:3000/cruises", "label": "Сайт"})).unwrap();
    assert_eq!(r["status"], "set");
    assert_eq!(st.get_thread(&t.id).unwrap().meta["preview"], json!({"url": "http://localhost:3000/cruises", "label": "Сайт"}));

    // an outside address is refused and the previous button stays
    assert!(tools::call(&st, &ctx, "senate_preview", &json!({"url": "https://evil.example"})).is_err());
    assert_eq!(st.get_thread(&t.id).unwrap().meta["preview"]["url"], "http://localhost:3000/cruises");

    // no label: the address is the label
    tools::call(&st, &ctx, "senate_preview", &json!({"url": "http://127.0.0.1:5173"})).unwrap();
    assert_eq!(st.get_thread(&t.id).unwrap().meta["preview"]["label"], "127.0.0.1:5173");

    tools::call(&st, &ctx, "senate_preview", &json!({"clear": true})).unwrap();
    assert!(st.get_thread(&t.id).unwrap().meta.get("preview").is_none());

    // a run without a chat has nowhere to show it
    let lone = ToolCtx { agent_id: dev.id.clone(), ..Default::default() };
    assert!(tools::call(&st, &lone, "senate_preview", &json!({"url": "http://localhost:3000"})).is_err());
}

#[test]
fn messages_to_a_busy_specialist_wait_in_line_others_run_in_parallel() {
    let (st, dev, arch, p) = setup();
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), ..Default::default() }).unwrap();
    let say = |to: &Agent, text: &str| st.send_chat(ChatSend { thread_id: t.id.clone(), text: format!("@{} {}", to.slug, text), ..Default::default() }, "mock").unwrap().sessions[0].clone();
    let d1 = say(&dev, "первое");
    let d2 = say(&dev, "второе");
    let d3 = say(&dev, "третье");
    let a1 = say(&arch, "вопрос");

    // first in line starts; the developer's other messages wait, the architect is not held up
    let first = st.claim_session("n", &|_| true).unwrap().unwrap();
    assert_eq!(first.id, d1.id);
    let next = st.claim_session("n", &|_| true).unwrap().unwrap();
    assert_eq!(next.id, a1.id, "another specialist runs in parallel");
    assert!(st.claim_session("n", &|_| true).unwrap().is_none(), "the developer is busy: second and third wait");

    // when the run ends, the next message goes — in the order it was written
    st.finish_session(&d1.id, SessionStatus::Done, "", "", "", 0, 0).unwrap();
    assert_eq!(st.claim_session("n", &|_| true).unwrap().unwrap().id, d2.id);
    assert!(st.claim_session("n", &|_| true).unwrap().is_none());
    st.finish_session(&d2.id, SessionStatus::Done, "", "", "", 0, 0).unwrap();
    assert_eq!(st.claim_session("n", &|_| true).unwrap().unwrap().id, d3.id);

    // another chat with the same specialist is a separate line
    let t2 = st.create_thread(NewThread { project_id: Some(p.id.clone()), ..Default::default() }).unwrap();
    let other = st.send_chat(ChatSend { thread_id: t2.id.clone(), text: format!("@{} в другом чате", dev.slug), ..Default::default() }, "mock").unwrap().sessions[0].clone();
    assert_eq!(st.claim_session("n", &|_| true).unwrap().unwrap().id, other.id);
}

#[test]
fn a_plan_the_person_disagrees_with_is_held_and_reaches_the_specialist_once() {
    let (st, dev, _, p) = setup();
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), agent_id: Some(dev.id.clone()), ..Default::default() }).unwrap();
    let plan = json!([
        {"content": "Каркас и данные", "status": "completed"},
        {"content": "Отправка заявок на почту", "status": "in_progress"},
        {"content": "Деплой", "status": "bogus"},
        {"content": "   ", "status": "pending"},
        "not an object"
    ]);
    let hold = st.hold_plan(&t.id, &plan).unwrap();
    assert_eq!(hold["todos"].as_array().unwrap().len(), 3, "blank and malformed items are dropped");
    assert_eq!(hold["todos"][2]["status"], "pending", "an unknown status becomes pending");
    assert!(st.get_thread(&t.id).unwrap().meta.get("plan_hold").is_some());

    // the next message carries the objection and the plan as it stood
    let d = st.send_chat(ChatSend { thread_id: t.id.clone(), text: "Почту не надо, пусть заявки идут в CRM".into(), ..Default::default() }, "mock").unwrap();
    let prompt = &d.sessions[0].prompt;
    assert!(prompt.contains("План приостановлен человеком"), "{prompt}");
    assert!(prompt.contains("[в работе] Отправка заявок на почту") && prompt.contains("[готово] Каркас и данные"), "{prompt}");
    assert!(prompt.ends_with("Почту не надо, пусть заявки идут в CRM"));

    // ...and only that one: the hold is spent
    assert!(st.get_thread(&t.id).unwrap().meta.get("plan_hold").is_none());
    let d2 = st.send_chat(ChatSend { thread_id: t.id.clone(), text: "Спасибо".into(), ..Default::default() }, "mock").unwrap();
    assert!(!d2.sessions[0].prompt.contains("План приостановлен"));

    // changing one's mind: the plan goes on, nothing is sent to the specialist
    st.hold_plan(&t.id, &plan).unwrap();
    st.release_plan(&t.id).unwrap();
    assert!(st.get_thread(&t.id).unwrap().meta.get("plan_hold").is_none());
}

#[test]
fn naming_a_specialist_beats_the_chat_they_are_pinned_to() {
    let (st, dev, arch, p) = setup();
    // a chat pinned to the architect
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), agent_id: Some(arch.id.clone()), ..Default::default() }).unwrap();
    let busy = st.send_chat(ChatSend { thread_id: t.id.clone(), text: "Продумай модули".into(), ..Default::default() }, "mock").unwrap();
    assert_eq!(busy.sessions[0].agent_id, arch.id, "no name: the pinned specialist answers");
    let first = st.claim_session("n", &|_| true).unwrap().unwrap();
    assert_eq!(first.agent_id, arch.id);

    // the architect is working; a message to the developer does not wait for him
    let to_dev = st.send_chat(ChatSend { thread_id: t.id.clone(), text: format!("@{} глянь баг в форме", dev.slug), ..Default::default() }, "mock").unwrap();
    assert_eq!(to_dev.sessions.len(), 1);
    assert_eq!(to_dev.sessions[0].agent_id, dev.id);
    assert_eq!(st.claim_session("n", &|_| true).unwrap().unwrap().agent_id, dev.id, "starts at once, in parallel");
}

#[test]
fn tasks_of_others_are_handled_on_the_persons_behalf_inside_their_project_only() {
    let (st, dev, arch, p) = setup();
    let other_project = st.create_project(NewProject { name: "Другой".into(), goal: "x".into(), members: vec![dev.id.clone()], ..Default::default() }).unwrap();
    let assistant = st.create_agent(NewAgent { name: "Помощник".into(), role: "assistant".into(), ..Default::default() }).unwrap();
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), ..Default::default() }).unwrap();
    let ctx = ToolCtx { agent_id: assistant.id.clone(), thread_id: Some(t.id.clone()), project_id: Some(p.id.clone()), ..Default::default() };

    let theirs = obl(&st, &arch, &p, ""); // the architect's task in this project
    let ownerless = st.create_obligation(NewObligation { title: "Без исполнителя".into(), result: "r".into(), project_id: Some(p.id.clone()), ..Default::default() }).unwrap();
    let foreign = obl(&st, &arch, &other_project, "");

    // by default, someone else's task is the person's call: it waits for approval and nothing changes
    let r = tools::call(&st, &ctx, "senate_obligation_update", &json!({"id": theirs.id, "state": "cancelled", "outcome": "не нужна"})).unwrap();
    assert_eq!(r["status"], "pending_approval");
    assert_eq!(st.get_obligation(&theirs.id).unwrap().state, ObligationState::Accepted);
    let ap = st.list_approvals(true).unwrap().remove(0);
    assert_eq!(ap.action, "obligation.manage");
    st.decide_approval(&ap.id, true).unwrap();
    assert_eq!(st.get_obligation(&theirs.id).unwrap().state, ObligationState::Cancelled, "the approved action ran once");

    // with the grant the assistant works without asking: another's task, a task nobody owns
    st.set_grant(Some(&assistant.id), None, action::OBLIGATION_MANAGE, GrantMode::Auto).unwrap();
    let t2 = obl(&st, &arch, &p, "");
    assert_eq!(tools::call(&st, &ctx, "senate_obligation_update", &json!({"id": t2.id, "state": "cancelled", "outcome": "дубль"})).unwrap()["state"], "cancelled");
    assert_eq!(tools::call(&st, &ctx, "senate_obligation_update", &json!({"id": ownerless.id, "state": "cancelled", "outcome": "устарела"})).unwrap()["state"], "cancelled");

    // the grant is the person's trust in this specialist, whichever project the task is in
    assert_eq!(tools::call(&st, &ctx, "senate_obligation_update", &json!({"id": foreign.id, "state": "cancelled", "outcome": "x"})).unwrap()["state"], "cancelled");
    // the whole picture, with owners and projects by name
    let all = tools::call(&st, &ctx, "senate_obligations", &json!({"scope": "all"})).unwrap();
    assert!(all.as_array().unwrap().iter().all(|o| o["project"].is_string() && o["owner"].is_string()));
    // a batch: one change for several, one answer each, a failure does not stop the rest
    let b1 = obl(&st, &arch, &p, "");
    let b2 = obl(&st, &arch, &other_project, "");
    let r = tools::call(&st, &ctx, "senate_obligation_update", &json!({"ids": [b1.id, "nope", b2.id], "state": "cancelled", "outcome": "уборка"})).unwrap();
    let res = r["results"].as_array().unwrap();
    assert_eq!(res.len(), 3);
    assert_eq!(res[0]["result"]["state"], "cancelled");
    assert!(res[1]["error"].is_string());
    assert_eq!(res[2]["result"]["state"], "cancelled");
    // listing everything is for those who may manage; others only see their own
    let stranger = ToolCtx { agent_id: dev.id.clone(), thread_id: Some(t.id.clone()), project_id: Some(p.id.clone()), ..Default::default() };
    st.set_grant(Some(&dev.id), None, action::OBLIGATION_MANAGE, GrantMode::Deny).unwrap();
    assert!(tools::call(&st, &stranger, "senate_obligations", &json!({"scope": "all"})).is_err());

    // history shows what happened; handing over works for another's task too
    let h = tools::call(&st, &ctx, "senate_obligation_history", &json!({"id": t2.id})).unwrap();
    assert_eq!(h["state"], "cancelled");
    assert!(h["events"].as_array().unwrap().len() >= 2);
    let t3 = obl(&st, &arch, &p, "");
    let r = tools::call(&st, &ctx, "senate_obligation_reassign", &json!({"id": t3.id, "owner": dev.slug})).unwrap();
    assert_eq!(r["owner"], dev.slug.as_str());
    assert_eq!(st.get_obligation(&t3.id).unwrap().owner_id.as_deref(), Some(dev.id.as_str()));
}

#[test]
fn the_agent_is_shown_what_the_person_attached() {
    let (st, dev, _, p) = setup();
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), agent_id: Some(dev.id.clone()), ..Default::default() }).unwrap();
    let shot = json!({"id": "a1", "name": "скрин.png", "type": "image/png", "size": 20480, "url": "/api/uploads/a1/x", "path": "C:/Senate/uploads/a1/скрин.png"});
    // a picture with a caption
    let d = st.send_chat(ChatSend { thread_id: t.id.clone(), text: "Что не так с версткой?".into(), attachments: vec![shot.clone()], ..Default::default() }, "mock").unwrap();
    let prompt = &d.sessions[0].prompt;
    assert!(prompt.contains("C:/Senate/uploads/a1/скрин.png") && prompt.contains("инструментом Read"), "{prompt}");
    assert_eq!(d.user_message.props["attachments"][0]["name"], "скрин.png", "kept with the message");
    // a picture alone, without a word
    let d2 = st.send_chat(ChatSend { thread_id: t.id.clone(), text: String::new(), attachments: vec![shot], ..Default::default() }, "mock").unwrap();
    assert!(d2.sessions[0].prompt.contains("смотри вложения"));
    // an entry without a path (not resolved by the node) tells the agent nothing
    let d3 = st.send_chat(ChatSend { thread_id: t.id.clone(), text: "x".into(), attachments: vec![json!({"name": "a.png"})], ..Default::default() }, "mock").unwrap();
    assert!(!d3.sessions[0].prompt.contains("senate-attachments"));
}

#[test]
fn a_specialist_always_works_on_their_own_model() {
    let (st, dev, arch, p) = setup();
    st.update_agent_basics(&dev.id, None, None, Some("claude:claude-sonnet-5-5")).unwrap();
    st.update_agent_basics(&arch.id, None, None, Some("claude:opus")).unwrap();
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), agent_id: Some(dev.id.clone()), ..Default::default() }).unwrap();
    // a model left on the chat by an older version, and one sent by an old client: neither counts
    st.patch_thread(&t.id, senate_core::store::ThreadPatch { model: Some("codex:gpt-6.1-sol".into()), ..Default::default() }).unwrap();

    let own = st.send_chat(ChatSend { thread_id: t.id.clone(), text: "Поправь баг".into(), model: Some("kimi:x".into()), ..Default::default() }, "mock").unwrap();
    assert_eq!(own.sessions[0].backend, "claude:claude-sonnet-5-5");
    let other = st.send_chat(ChatSend { thread_id: t.id.clone(), text: format!("@{} посмотри структуру", arch.slug), ..Default::default() }, "mock").unwrap();
    assert_eq!(other.sessions[0].backend, "claude:opus");
    assert_eq!(st.get_thread(&t.id).unwrap().model, "codex:gpt-6.1-sol", "the old value is not rewritten either");

    // no model of their own: the default of the node
    let plain = st.create_agent(NewAgent { name: "Без модели".into(), role: "custom".into(), ..Default::default() }).unwrap();
    let t2 = st.create_thread(NewThread { agent_id: Some(plain.id.clone()), ..Default::default() }).unwrap();
    let d = st.send_chat(ChatSend { thread_id: t2.id.clone(), text: "привет".into(), ..Default::default() }, "claude:haiku").unwrap();
    assert_eq!(d.sessions[0].backend, "claude:haiku");
}

#[test]
fn one_specialist_one_cli_session() {
    let (st, dev, arch, p) = setup();
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), agent_id: Some(dev.id.clone()), ..Default::default() }).unwrap();
    let fresh = |st: &Store| st.get_thread(&t.id).unwrap();

    // each specialist gets, and keeps, a session of their own — on the same provider, in the same chat
    st.set_cli_session_for(&t.id, "codex", &arch.id, "sess-architect").unwrap();
    st.set_cli_session_for(&t.id, "codex", &dev.id, "sess-developer").unwrap();
    assert_eq!(st.cli_session_for(&fresh(&st), "codex", &arch.id).as_deref(), Some("sess-architect"));
    assert_eq!(st.cli_session_for(&fresh(&st), "codex", &dev.id).as_deref(), Some("sess-developer"));

    // a third one does not inherit anybody's session: it starts its own
    let designer = st.create_agent(NewAgent { name: "Дизайнер".into(), role: "designer".into(), ..Default::default() }).unwrap();
    assert_eq!(st.cli_session_for(&fresh(&st), "codex", &designer.id), None);
    // and sessions of another provider are another thing again
    assert_eq!(st.cli_session_for(&fresh(&st), "claude", &dev.id), None);

    // a chat from before (one session per provider) hands it to the specialist the chat is pinned to, nobody else
    let old = st.create_thread(NewThread { project_id: Some(p.id.clone()), agent_id: Some(dev.id.clone()), ..Default::default() }).unwrap();
    st.set_thread_cli_session(&old.id, "codex", "legacy").unwrap();
    let old = st.get_thread(&old.id).unwrap();
    assert_eq!(st.cli_session_for(&old, "codex", &dev.id).as_deref(), Some("legacy"));
    assert_eq!(st.cli_session_for(&old, "codex", &arch.id), None, "a specialist the chat is not pinned to never takes it over");
}

#[test]
fn a_specialist_gets_a_servers_secret_only_as_the_person_allowed() {
    let (st, dev, arch, p) = setup();
    let t = st.create_thread(NewThread { project_id: Some(p.id.clone()), agent_id: Some(dev.id.clone()), ..Default::default() }).unwrap();
    let ctx = |a: &Agent| ToolCtx { agent_id: a.id.clone(), thread_id: Some(t.id.clone()), project_id: Some(p.id.clone()), ..Default::default() };
    let acc = st
        .save_access(
            None,
            AccessInput {
                name: Some("prod-vps".into()), label: Some("Боевой сервер".into()), kind: Some("server".into()),
                host: Some("ssh://deploy@203.0.113.7:22".into()), login: Some("deploy".into()), secret: Some("s3cr3t-key".into()),
                note: Some("Деплой через ssh".into()),
                grants: Some([(dev.id.clone(), "ask".to_string())].into_iter().collect()),
                ..Default::default()
            },
        )
        .unwrap();
    // what the API shows never holds the secret
    assert!(!acc.view().to_string().contains("s3cr3t"), "{}", acc.view());
    assert_eq!(acc.view()["has_secret"], true);

    // the list tells each specialist their own standing; an address is for those who have the access
    let mine = tools::call(&st, &ctx(&dev), "senate_access_list", &json!({})).unwrap();
    assert_eq!(mine[0]["yours"], "выдаётся с согласия человека");
    assert_eq!(mine[0]["host"], "ssh://deploy@203.0.113.7:22");
    let theirs = tools::call(&st, &ctx(&arch), "senate_access_list", &json!({})).unwrap();
    assert_eq!(theirs[0]["yours"], "не выдан");
    assert_eq!(theirs[0]["host"], "");
    assert!(!mine.to_string().contains("s3cr3t") && !theirs.to_string().contains("s3cr3t"), "the list has no secrets");

    // no access: refused, and the attempt is written down
    let e = tools::call(&st, &ctx(&arch), "senate_access_get", &json!({"name": "prod-vps", "reason": "выкатить"})).unwrap_err();
    assert!(matches!(e, senate_core::Error::Forbidden(_)), "{e}");

    // "ask": the first call waits for the person; nothing leaks while it waits
    let r = tools::call(&st, &ctx(&dev), "senate_access_get", &json!({"name": "prod-vps", "reason": "выкатить релиз"})).unwrap();
    assert_eq!(r["status"], "pending_approval");
    assert!(!r.to_string().contains("s3cr3t"));
    let ap = st.list_approvals(true).unwrap().remove(0);
    assert_eq!(ap.action, "access.use");
    st.decide_approval(&ap.id, true).unwrap();
    // after the yes: the next call gets it, once
    let got = tools::call(&st, &ctx(&dev), "senate_access_get", &json!({"name": "prod-vps", "reason": "выкатить релиз"})).unwrap();
    assert_eq!(got["secret"], "s3cr3t-key");
    assert_eq!(got["login"], "deploy");
    let again = tools::call(&st, &ctx(&dev), "senate_access_get", &json!({"name": "prod-vps"})).unwrap();
    assert_eq!(again["status"], "pending_approval", "one yes is one use");
    // a no gives nobody anything
    let ap2 = st.list_approvals(true).unwrap().remove(0);
    st.decide_approval(&ap2.id, false).unwrap();
    assert!(tools::call(&st, &ctx(&dev), "senate_access_get", &json!({"name": "prod-vps"})).unwrap()["status"] == "pending_approval");

    // "allow": handed over at once; switching the access off stops it
    st.save_access(Some(&acc.id), AccessInput { grants: Some([(dev.id.clone(), "allow".to_string())].into_iter().collect()), ..Default::default() }).unwrap();
    assert_eq!(tools::call(&st, &ctx(&dev), "senate_access_get", &json!({"name": "prod-vps", "reason": "логи"})).unwrap()["secret"], "s3cr3t-key");
    st.save_access(Some(&acc.id), AccessInput { enabled: Some(false), ..Default::default() }).unwrap();
    assert!(tools::call(&st, &ctx(&dev), "senate_access_get", &json!({"name": "prod-vps"})).is_err());

    // the secret is kept when the rest is edited, and gone only when the person says so
    st.save_access(Some(&acc.id), AccessInput { enabled: Some(true), note: Some("новое".into()), secret: Some(String::new()), ..Default::default() }).unwrap();
    assert_eq!(st.get_access("prod-vps").unwrap().secret, "s3cr3t-key");
    st.save_access(Some(&acc.id), AccessInput { clear_secret: Some(true), ..Default::default() }).unwrap();
    assert_eq!(st.get_access("prod-vps").unwrap().view()["has_secret"], false);

    // every use is on the record: who, what for — not the secret
    let log = st.access_log(&acc.id, 50).unwrap();
    assert!(log.iter().any(|e| e.kind == "access.used" && e.actor == dev.id && e.payload["reason"] == "выкатить релиз"), "{log:?}");
    assert!(log.iter().any(|e| e.payload["how"] == "отказано"));
    assert!(!format!("{log:?}").contains("s3cr3t"));

    // names and kinds are checked
    assert!(st.save_access(None, AccessInput { name: Some("Bad Name".into()), host: Some("x".into()), ..Default::default() }).is_err());
    assert!(st.save_access(None, AccessInput { name: Some("ok-name".into()), host: Some("x".into()), kind: Some("nuclear".into()), ..Default::default() }).is_err());
    assert!(st.save_access(None, AccessInput { name: Some("ok-name".into()), host: Some(String::new()), ..Default::default() }).is_err());
}

#[test]
fn routines_make_a_task_when_due_and_never_pile_up() {
    let (st, dev, _, _) = setup();
    let hour = 3600 * 1000;
    let r = st
        .save_routine(None, RoutineInput { agent_id: Some(dev.id.clone()), name: Some("Понедельник".into()), cron: Some("0 9 * * 1".into()), title: Some("SEO: понедельничный сценарий".into()), prompt: Some("Разбери пакет данных за неделю".into()), ..Default::default() }, 3 * 3600)
        .unwrap();
    assert!(r.next_run_at.unwrap() > senate_core::now_ms(), "the next time is in the future");
    assert!(r.enabled && r.last_run_at.is_none());
    // a bad time is explained, a name is unique per specialist
    assert!(st.save_routine(None, RoutineInput { agent_id: Some(dev.id.clone()), name: Some("Плохая".into()), cron: Some("61 * * * *".into()), ..Default::default() }, 0).is_err());
    assert!(st.save_routine(None, RoutineInput { agent_id: Some(dev.id.clone()), name: Some("Понедельник".into()), cron: Some("0 9 * * 2".into()), ..Default::default() }, 0).is_err());

    // not yet due: nothing
    assert!(st.fire_due_routines(senate_core::now_ms(), 3 * 3600).unwrap().is_empty());
    // due: a task for the specialist with the instruction, the date in the title, and the next time set
    let due_at = r.next_run_at.unwrap();
    let fired = st.fire_due_routines(due_at + 60_000, 3 * 3600).unwrap();
    assert_eq!(fired.len(), 1);
    let o = &fired[0].1;
    assert_eq!(o.owner_id.as_deref(), Some(dev.id.as_str()));
    assert!(o.title.starts_with("SEO: понедельничный сценарий · 20"), "{}", o.title);
    assert!(o.result.contains("Разбери пакет данных"));
    assert_eq!(o.state, ObligationState::Accepted, "the scheduler takes it from here");
    let after = st.get_routine(&r.id).unwrap();
    assert!(after.next_run_at.unwrap() > due_at && after.last_run_at.is_some());

    // the next one comes while the last is still open: skipped, not stacked
    let next_due = after.next_run_at.unwrap();
    assert!(st.fire_due_routines(next_due + 60_000, 3 * 3600).unwrap().is_empty());
    assert!(st.events(Some("routine"), Some(&r.id), 10).unwrap().iter().any(|e| e.kind == "routine.skipped" && e.payload["why"].as_str().unwrap().contains("ещё не завершён")));
    // finish it: the one after fires
    st.transition(&o.id, ObligationState::Running, Default::default(), "system").unwrap();
    st.transition(&o.id, ObligationState::Done, TransitionArgs { outcome: Some("отчёт готов".into()), ..Default::default() }, &dev.id).unwrap();
    let nd = st.get_routine(&r.id).unwrap().next_run_at.unwrap();
    assert_eq!(st.fire_due_routines(nd + 60_000, 3 * 3600).unwrap().len(), 1);

    // the app was off for days: that run is skipped, not made up
    let nd2 = st.get_routine(&r.id).unwrap().next_run_at.unwrap();
    let open = st.list_obligations(Some(&dev.id), None, true).unwrap();
    for o in open {
        st.transition(&o.id, ObligationState::Running, Default::default(), "system").ok();
        st.transition(&o.id, ObligationState::Done, TransitionArgs { outcome: Some("x".into()), ..Default::default() }, &dev.id).ok();
    }
    assert!(st.fire_due_routines(nd2 + 48 * hour, 3 * 3600).unwrap().is_empty());
    assert!(st.get_routine(&r.id).unwrap().next_run_at.unwrap() > nd2 + 48 * hour);

    // switched off: silent. switched on again: the next time is worked out afresh
    let off = st.save_routine(Some(&r.id), RoutineInput { enabled: Some(false), ..Default::default() }, 3 * 3600).unwrap();
    assert!(off.next_run_at.is_none());
    assert!(st.fire_due_routines(nd2 + 400 * hour, 0).unwrap().is_empty());
    assert!(st.save_routine(Some(&r.id), RoutineInput { enabled: Some(true), ..Default::default() }, 3 * 3600).unwrap().next_run_at.is_some());

    // a run by hand does not move the schedule
    let before = st.get_routine(&r.id).unwrap().next_run_at;
    st.run_routine(&r.id, senate_core::now_ms(), 0).unwrap();
    assert_eq!(st.get_routine(&r.id).unwrap().next_run_at, before);
}

#[test]
fn routines_declared_by_a_skill_are_made_once_and_the_persons_changes_stay() {
    let (st, dev, _, _) = setup();
    let (r, made) = st.ensure_routine(&dev.id, "Четверг", "0 9 * * 4", "SEO: четверг", "Разбери эффект внедрённого", "skill:seo-wizard", 0).unwrap();
    assert!(made && r.source == "skill:seo-wizard");
    // the person moves it to Friday and later switches it off
    st.save_routine(Some(&r.id), RoutineInput { cron: Some("0 10 * * 5".into()), enabled: Some(false), ..Default::default() }, 0).unwrap();
    // the next start declares it again: nothing changes
    let (again, made_again) = st.ensure_routine(&dev.id, "Четверг", "0 9 * * 4", "SEO: четверг", "Разбери эффект внедрённого", "skill:seo-wizard", 0).unwrap();
    assert!(!made_again);
    assert_eq!(again.cron, "0 10 * * 5");
    assert!(!again.enabled);
    assert_eq!(st.list_routines(Some(&dev.id)).unwrap().len(), 1);
}

#[test]
fn the_calendar_lays_out_the_plan_what_was_done_and_deadlines() {
    let (st, dev, arch, p) = setup();
    let day = 86_400_000i64;
    let now = senate_core::now_ms();
    // two schedules: every weekday at 09:00 and the 10th, 20th, 25th
    st.save_routine(None, RoutineInput { agent_id: Some(dev.id.clone()), name: Some("Будни".into()), cron: Some("0 9 * * 1-5".into()), title: Some("Обход".into()), ..Default::default() }, 0).unwrap();
    let monthly = st.save_routine(None, RoutineInput { agent_id: Some(arch.id.clone()), name: Some("Месяц".into()), cron: Some("0 9 10,20,25 * *".into()), title: Some("Итоги".into()), ..Default::default() }, 0).unwrap();
    let cal = st.calendar(now, now + 62 * day, 0).unwrap();
    let items = cal["items"].as_array().unwrap();
    let planned: Vec<&serde_json::Value> = items.iter().filter(|i| i["kind"] == "planned").collect();
    // about 44 weekdays and 6 monthly days in nine weeks: both schedules are there, in time order, never in the past
    assert!(planned.iter().filter(|i| i["name"] == "Будни").count() >= 40);
    assert!(planned.iter().filter(|i| i["name"] == "Месяц").count() >= 5);
    assert!(planned.iter().all(|i| i["at"].as_i64().unwrap() >= now));
    let ats: Vec<i64> = items.iter().map(|i| i["at"].as_i64().unwrap()).collect();
    assert!(ats.windows(2).all(|w| w[0] <= w[1]), "sorted");

    // a disabled schedule is not in the plan
    st.save_routine(Some(&monthly.id), RoutineInput { enabled: Some(false), ..Default::default() }, 0).unwrap();
    let cal = st.calendar(now, now + 62 * day, 0).unwrap();
    assert!(cal["items"].as_array().unwrap().iter().all(|i| i["name"] != "Месяц"));

    // what a schedule made shows as a run with its state; a task with a deadline as a deadline
    let weekdays = st.list_routines(Some(&dev.id)).unwrap().remove(0);
    let made = st.run_routine(&weekdays.id, now, 0).unwrap();
    let due = st.create_obligation(NewObligation { title: "Сдать макет".into(), result: "макет".into(), owner_id: Some(arch.id.clone()), project_id: Some(p.id.clone()), due_at: Some(now + 3 * day), ..Default::default() }).unwrap();
    let cal = st.calendar(now - day, now + 10 * day, 0).unwrap();
    let items = cal["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["kind"] == "run" && i["obligation_id"] == made.id.as_str() && i["state"] == "accepted"));
    assert!(items.iter().any(|i| i["kind"] == "due" && i["obligation_id"] == due.id.as_str()));
}

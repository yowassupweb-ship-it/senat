//! Working set (Senate.md section 7): what a session gets at start. Context is a resource, not a
//! container for the specialist's whole history — only what the work needs, with pointers to sources.

use crate::error::Result;
use crate::model::*;
use crate::store::Store;

const MEMORY_IN_PROMPT: i64 = 8;
const MAX_ENTRY_CHARS: usize = 600;

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// Who answers a message that does not name a single addressee: @mentions first, then topic words.
/// Deterministic on purpose — routing must be predictable and cost no tokens.
/// The specialists the text addresses by name (`@slug` or `@Имя`). Explicit: it beats a chat pinned to someone.
pub fn mentioned(text: &str, candidates: &[Agent]) -> Vec<Agent> {
    let lower = text.to_lowercase();
    candidates
        .iter()
        .filter(|a| lower.contains(&format!("@{}", a.slug)) || lower.contains(&format!("@{}", a.name.to_lowercase())))
        .cloned()
        .collect()
}

pub fn route(text: &str, candidates: &[Agent]) -> Vec<Agent> {
    let mut out = mentioned(text, candidates);
    if !out.is_empty() {
        return out;
    }
    let lower = text.to_lowercase();
    const TOPICS: &[(&str, &[&str])] = &[
        ("designer", &["дизайн", "макет", "интерфейс", "ui", "ux", "цвет", "шрифт", "вёрстк", "верстк"]),
        ("developer", &["код", "баг", "ошибк", "реализ", "функци", "тест", "компил", "рефактор", "git", "сборк"]),
        ("analyst", &["данные", "данных", "анализ", "метрик", "гипотез", "статистик", "отчёт", "отчет", "график"]),
        ("business", &["стоимост", "бюджет", "бизнес", "клиент", "договор", "цена", "рынок"]),
        ("architect", &["архитектур", "систем", "зависимост", "модул", "структур", "проектир"]),
    ];
    let mut best: Option<(&str, usize)> = None;
    for (role, words) in TOPICS {
        let n = words.iter().filter(|w| lower.contains(*w)).count();
        if n > 0 && best.map(|b| n > b.1).unwrap_or(true) {
            best = Some((role, n));
        }
    }
    if let Some((role, _)) = best {
        if let Some(a) = candidates.iter().find(|a| a.role == role) {
            out.push(a.clone());
            return out;
        }
    }
    candidates.first().cloned().into_iter().collect()
}

fn memory_block(mem: &[Memory]) -> String {
    if mem.is_empty() {
        return String::new();
    }
    let mut s = String::from("## Релевантный опыт\n");
    for m in mem {
        let tag = match m.kind {
            MemoryKind::Verified => "проверено",
            MemoryKind::Hypothesis => "гипотеза",
            MemoryKind::Observation => "наблюдение",
            MemoryKind::Decision => "решение",
            MemoryKind::Preference => "предпочтение",
            MemoryKind::Error => "ошибка (не повторять)",
        };
        let layer = match (m.level, &m.company_id) {
            (MemoryLevel::Company, _) => " · компания",
            (MemoryLevel::Personal, Some(_)) => " · моё о компании",
            _ => "",
        };
        let src = if m.source.is_empty() { String::new() } else { format!(" · источник: {}", clip(&m.source, 80)) };
        s += &format!("- [{tag}{layer}{src}] {} (id {})\n", clip(&m.body, MAX_ENTRY_CHARS), &m.id[..8]);
    }
    s
}

fn company_block(store: &Store, company_id: &str) -> Result<String> {
    let c = store.get_company(company_id)?;
    let mut s = format!("## Компания «{}»\n", c.name);
    if !c.brief.trim().is_empty() {
        s += &format!("{}\n", c.brief.trim());
    }
    let decisions: Vec<Decision> = store.list_company_decisions(company_id)?.into_iter().filter(|d| d.status == DecisionStatus::Accepted).take(15).collect();
    if !decisions.is_empty() {
        s += "Общие решения компании (действуют во всех её проектах):\n";
        for d in decisions {
            s += &format!("- {}{}\n", d.title, if d.body.is_empty() { String::new() } else { format!(": {}", clip(&d.body, 300)) });
        }
    }
    s += "\n";
    Ok(s)
}

fn project_block(store: &Store, p: &Project) -> Result<String> {
    let mut s = format!("## Проект «{}»\n", p.name);
    if !p.goal.is_empty() {
        s += &format!("Цель: {}\n", p.goal);
    }
    if !p.criteria.is_empty() {
        s += &format!("Критерии результата: {}\n", p.criteria);
    }
    if !p.constraints.is_empty() {
        s += &format!("Ограничения: {}\n", p.constraints);
    }
    let decisions: Vec<Decision> = store.list_decisions(&p.id)?.into_iter().filter(|d| d.status == DecisionStatus::Accepted).take(12).collect();
    if !decisions.is_empty() {
        s += "Принятые решения:\n";
        for d in decisions {
            s += &format!("- {}{}\n", d.title, if d.body.is_empty() { String::new() } else { format!(": {}", clip(&d.body, 300)) });
        }
    }
    Ok(s)
}

const PROTOCOL: &str = "\
## Как работать с человеком
- Не принимай просьбы и правки на веру. Прежде чем действовать, убедись, что понял задачу и её цель. Если от ответа зависит результат — задай 1–3 конкретных вопроса и дождись ответа. Но не спрашивай того, что можешь выяснить сам (файлы, код, память, `senate_recall`).
- Если просьба противоречит принятым решениям, цели проекта или здравому смыслу — возрази и назови основание. Согласие без проверки — не услуга.
- Всё, что можешь добыть сам, добывай сразу и отвечай по результату, а не предлагай «если хотите, могу»: файлы и папка проекта, сайт компании из её описания, документация, память (`senate_recall`). Спрашивай человека только о том, чего получить нельзя.
- Человек называет компанию, проект или клиента — значит, он уже дал тебе всё нужное: не переспрашивай, где искать. Среда сама находит названное и привязывает разговор; если нужно больше — `senate_find` (поиск по компаниям, проектам и их памяти), `senate_attach` (привязать разговор), `senate_recall` с компанией или проектом.
- Если разговор не привязан ни к проекту, ни к компании, а работа нетривиальна или будет продолжаться, спроси, завести ли проект (и в какой компании). Только после согласия вызови `senate_project_create`.
- Если вопрос слишком обширный для одного специалиста, предложи вынести его на форум — обсуждение с коллегами и план работ.
- В конце работы коротко скажи, что ты запомнил и что осталось открытым.

## Как работать в Senate
- Ты — постоянный специалист: профиль и опыт сохраняются между разговорами, контекст этой сессии — нет.
- Инструменты `senate_*` — способ записать в среду то, что должно пережить сессию: знание (`senate_remember`), \
задачо (`senate_obligation_*`), решение (`senate_decision`). Не пересказывай им разговор — только существенное.
- Сообщение «готово» без результата не принимается: у завершённой задачи должен быть описанный результат.
- После содержательной работы сам запиши 1–3 вывода в память (`senate_remember`): устойчивый факт, предпочтение человека, принятое решение с причиной, найденная ошибка, воспроизводимый приём. Если ты этого не сделаешь, среда извлечёт выводы сама — но твои формулировки точнее.
- Если не хватает сведений — сначала `senate_recall`, затем вопрос пользователю. Не выдумывай факты.
- Память у тебя слоистая: общее мастерство (`personal`) идёт с тобой во все компании; знание о конкретной компании кладёшь в `company_personal` (только тебе) или `company` (всем, кто там работает); не смешивай клиентов — то, что относится к одной компании, не переноси в другую.
- Решение, общее для всех проектов компании, предлагай со `scope: company`.
- Знание помечай честно: наблюдение, гипотеза или проверенное (с основанием проверки). Уверенность — не проверка.
- Права задаёт среда. Если инструмент ответил `pending_approval`, действие ждёт пользователя — продолжай остальное.
- Нужна картинка (иллюстрация, фон, заглушка, баннер) — сделай её `senate_image_generate`, а не ищи в интернете и не оставляй пустое место. Файл сохранится на этом компьютере, человек увидит его в чате, а путь придёт тебе в ответе.
- Нужна помощь коллеги по его части (SEO, дизайн, рассылки, код), и до результата недалеко — позови его `senate_call_colleague` (to — slug из `senate_colleagues`, request — суть с контекстом: он не видит твоих файлов и мыслей). Его ответ придёт к тебе отдельным ходом: свою работу можешь продолжать или закончить ход. Работа большая или с результатом — ставь задачу `senate_obligation_create`. Не зови ради вежливости и сначала посмотри, не занят ли он (`senate_colleagues`).
- Запустил локальный сервер разработки или страницу, которую человеку надо посмотреть, — вызови `senate_preview` с адресом (например, http://localhost:3000/cruises): под полем ввода появится кнопка. Поменялся порт или путь — вызови снова; остановил сервер — `senate_preview` с `clear: true`. Только адреса этого компьютера.
- Долгие команды (загрузка данных, сборка, тесты, всё, что идёт дольше пары минут) запускай в фоне и сразу занимайся следующим шагом. Не жди их циклами `until … sleep` или `while … sleep` в одной команде: человек не может тебе писать, пока ты ждёшь. Прогресс проверяй редкими короткими запросами (хвост лога), а если делать больше нечего — сообщи, что запущено и где смотреть результат, и закончи ход.
- Отвечай по-русски, по делу, без вступлений.";

/// Standing part of the prompt: identity, project picture, protocol. Goes into the system prompt.
pub fn system_prompt(store: &Store, agent: &Agent, project: Option<&Project>, company_id: Option<&str>) -> Result<String> {
    let mut s = format!("# {} — {}\n\n{}\n\n", agent.name, role_title(&agent.role), agent.profile.trim());
    if let Some(co) = company_id.or(project.and_then(|p| p.company_id.as_deref())) {
        s += &company_block(store, co)?;
    }
    if project.is_none() && company_id.is_none() {
        let companies: Vec<String> = store.list_companies()?.into_iter().take(12).map(|c| c.name).collect();
        let projects: Vec<String> = store.list_projects()?.into_iter().take(12).map(|p| p.name).collect();
        s += "## Контекст\nЭтот разговор пока не привязан ни к проекту, ни к компании.";
        if !companies.is_empty() {
            s += &format!(" Компании в среде: {}.", companies.join(", "));
        }
        if !projects.is_empty() {
            s += &format!(" Проекты: {}.", projects.join(", "));
        }
        s += "\n\n";
    }
    if let Some(p) = project {
        s += &project_block(store, p)?;
        if !p.root_path.is_empty() {
            s += &format!("Рабочая папка: {}\n", p.root_path);
        }
        let colleagues = store.project_members(&p.id)?;
        let others: Vec<String> = colleagues.iter().filter(|a| a.id != agent.id).map(|a| format!("{} (@{}, {})", a.name, a.slug, role_title(&a.role))).collect();
        if !others.is_empty() {
            s += &format!("Коллеги по проекту: {}\n", others.join("; "));
        }
        s += "\n";
    }
    s += PROTOCOL;
    Ok(s)
}

pub fn role_title(role: &str) -> &str {
    match role {
        "architect" => "архитектор",
        "developer" => "разработчик",
        "designer" => "дизайнер",
        "analyst" => "аналитик",
        "business" => "бизнес-специалист",
        "assistant" => "ассистент",
        _ => "специалист",
    }
}

/// Files the person attached to this message. The agent cannot see a picture it was not shown: it gets the path
/// on this machine and is told to open it. Only what the node itself resolved is here (see `uploads` in the node).
pub fn attachments_block(attachments: &[serde_json::Value]) -> String {
    let lines: Vec<String> = attachments
        .iter()
        .take(12)
        .filter_map(|a| {
            let path = a["path"].as_str().filter(|p| !p.is_empty())?;
            let name = a["name"].as_str().unwrap_or("файл");
            let kb = a["size"].as_u64().unwrap_or(0) / 1024;
            Some(format!("- {name} ({}, {kb} КБ): {path}", a["type"].as_str().unwrap_or("файл")))
        })
        .collect();
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "

<senate-attachments>
Человек приложил к сообщению файлы; они лежат на этом компьютере. Картинки и скриншоты открой инструментом Read по пути: он показывает изображение. Не описывай то, чего не открыл.
{}
</senate-attachments>",
        lines.join("
")
    )
}

/// The person stopped the specialist because they disagree with the plan (`plan_hold` in the chat's state).
/// The next message is an objection, not an instruction to carry on: say so, and show what was planned.
fn plan_hold_block(thread: &Thread) -> String {
    let Some(hold) = thread.meta.get("plan_hold") else { return String::new() };
    let mut s = String::from(
        "## План приостановлен человеком\n\
Человек остановил работу, потому что не согласен с твоим планом. Не продолжай его и ничего не меняй, пока не разберёшься. \
Прочитай его сообщение ниже: что именно не так. Если не хватает сведений — задай 1–2 вопроса. Затем покажи исправленный план \
(обнови список задач) и работай уже по нему. Не защищай прежний план из упрямства: согласие с человеком не требует отказа от здравого смысла, но решение за ним.\n\
План на момент остановки:\n",
    );
    for t in hold["todos"].as_array().map(|a| a.as_slice()).unwrap_or_default().iter().take(30) {
        let mark = match t["status"].as_str() {
            Some("completed") => "готово",
            Some("in_progress") => "в работе",
            _ => "ждёт",
        };
        s += &format!("- [{mark}] {}\n", t["content"].as_str().unwrap_or(""));
    }
    s += "\n";
    s
}

/// The user-turn prompt for a chat message: the text itself, preceded by a compact block of
/// relevant experience when there is any. Attachments and "where I am" context are added by the caller.
pub fn chat_turn_prompt(store: &Store, agent: &Agent, thread: &Thread, text: &str) -> Result<String> {
    let company = thread.company_id.clone().or(match &thread.project_id { Some(p) => store.company_of_project(p)?, None => None });
    let mem = store.search_memory_in(text, Some(&agent.id), thread.project_id.as_deref(), company.as_deref(), MEMORY_IN_PROMPT)?;
    let block = memory_block(&mem);
    // Open obligations are the agent's promises: remind, do not make the user repeat them.
    let open = store.list_obligations(Some(&agent.id), thread.project_id.as_deref(), true)?;
    let mut s = String::new();
    let named = mention_block(store, agent, thread, text)?;
    let hold = plan_hold_block(thread);
    if !block.is_empty() || !open.is_empty() || !named.is_empty() || !hold.is_empty() {
        s += "<senate-context>\n";
        s += &hold;
        s += &named;
        s += &block;
        if !open.is_empty() {
            s += "## Твои открытые задача\n";
            for o in open.iter().take(8) {
                s += &format!("- {} [{}] (id {}){}\n", o.title, o.state.as_str(), &o.id[..8], if o.next_step.is_empty() { String::new() } else { format!(" — дальше: {}", clip(&o.next_step, 120)) });
            }
        }
        s += "</senate-context>\n\n";
    }
    s += text;
    Ok(s)
}

/// Prompt for a session that exists because of an obligation (not a chat message).
pub fn build_obligation_prompt(store: &Store, agent: &Agent, o: &Obligation, reason: &str) -> Result<String> {
    let mut s = String::new();
    s += &format!("Ты продолжаешь работу над задачей.\nПричина запуска: {reason}.\n\n");
    s += &format!("## Задачо: {}\nСостояние: {}\n", o.title, o.state.as_str());
    if !o.result.is_empty() {
        s += &format!("Ожидаемый результат: {}\n", o.result);
    }
    if !o.criteria.is_empty() {
        s += &format!("Критерии завершения: {}\n", o.criteria);
    }
    if !o.next_step.is_empty() {
        s += &format!("Следующий шаг: {}\n", o.next_step);
    }
    if let Some(d) = o.due_at {
        s += &format!("Срок (unix мс): {d}\n");
    }
    s += &format!("id задача: {}\n\n", o.id);

    // Work log: what already happened, so a fresh session continues instead of starting over.
    let log = store.events(Some("obligation"), Some(&o.id), 12)?;
    if !log.is_empty() {
        s += "## Журнал работы (новые сверху)\n";
        for e in &log {
            let note = e.payload.get("note").and_then(|v| v.as_str()).unwrap_or("");
            s += &format!("- {}{}\n", e.kind, if note.is_empty() { String::new() } else { format!(": {}", clip(note, 200)) });
        }
        s += "\n";
    }
    let prev = store.list_sessions(None, Some(&o.id), Some(SessionStatus::Done), 3)?;
    for p in prev.iter().filter(|p| !p.summary.is_empty()) {
        s += &format!("Итог предыдущей сессии: {}\n", clip(&p.summary, 500));
    }
    let children = store.list_obligations(None, o.project_id.as_deref(), false)?.into_iter().filter(|c| c.parent_id.as_deref() == Some(&o.id)).collect::<Vec<_>>();
    if !children.is_empty() {
        s += "\n## Делегированное\n";
        for c in children {
            s += &format!("- {} [{}]{}\n", c.title, c.state.as_str(), if c.outcome.is_empty() { String::new() } else { format!(" → {}", clip(&c.outcome, 200)) });
        }
    }
    let company = match &o.project_id { Some(p) => store.company_of_project(p)?, None => None };
    let mem = store.search_memory_in(&format!("{} {}", o.title, o.result), Some(&agent.id), o.project_id.as_deref(), company.as_deref(), MEMORY_IN_PROMPT)?;
    let block = memory_block(&mem);
    if !block.is_empty() {
        s += "\n";
        s += &block;
    }
    s += "\nСделай следующий шаг. В конце обязательно обнови задачу инструментом `senate_obligation_update`: \
`done` (с описанием результата), `review`, `waiting` (с условием продолжения), `blocked` (с препятствием) \
или оставь `running` с `next_step`, если работа продолжится.\n";
    Ok(s)
}

fn outcome_text(o: &str) -> &'static str {
    match o {
        "decision" => "Принять решение по вопросу: что выбираем, почему, какие риски принимаем.",
        "analysis" => "Разобрать вопрос: варианты, аргументы за и против, что неизвестно.",
        _ => "Спланировать работу: что делать, в каком порядке, кто отвечает, как поймём, что готово.",
    }
}

/// The prompt of one speaker at a forum. `synthesis` = the leader's closing turn.
pub fn build_forum_prompt(store: &Store, agent: &Agent, thread: &Thread, round: i64, synthesis: bool) -> Result<String> {
    let meta = &thread.meta;
    let ids: Vec<String> = meta["participants"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut people = String::new();
    for id in &ids {
        if let Ok(a) = store.get_agent(id) {
            people += &format!("- {} (@{}, {})\n", a.name, a.slug, role_title(&a.role));
        }
    }
    let mut transcript = String::new();
    for m in store.list_messages(&thread.id, None, 80)? {
        if m.body.trim().is_empty() || m.author_kind == "system" {
            continue;
        }
        let who = match (m.author_kind.as_str(), m.author_id.as_deref()) {
            ("agent", Some(a)) => store.get_agent(a).map(|x| x.name).unwrap_or_else(|_| "Специалист".into()),
            _ => "Человек".into(),
        };
        transcript += &format!("**{who}:** {}\n\n", clip(m.body.trim(), 1500));
    }
    let mut s = format!(
        "Идёт ФОРУМ: коллеги обсуждают вопрос вместе. Ты — {} ({}).\n\n## Вопрос\n{}\n\n## Ожидаемый итог\n{}\n\n## Участники\n{}\n",
        agent.name,
        role_title(&agent.role),
        meta["topic"].as_str().unwrap_or(""),
        outcome_text(meta["outcome"].as_str().unwrap_or("plan")),
        people
    );
    if synthesis {
        s += "## Твоя задача\nТы подводишь итог форума для человека.\n\n\
Формат ответа:\n1. Короткий итог (до 150 слов): к чему пришли, где остались разногласия и почему.\n\
2. Затем блок ```senate-plan с JSON:\n\
{\"summary\":\"...\",\"decisions\":[{\"title\":\"...\",\"body\":\"...\",\"scope\":\"project|company\"}],\"work\":[{\"title\":\"...\",\"result\":\"ожидаемый результат\",\"criteria\":\"как поймём, что готово\",\"owner\":\"slug участника\",\"depends_on\":[\"title другой работы\"]}],\"open_questions\":[\"...\"]}\n\
Правила: в decisions — только то, с чем согласились; в work — конкретные поручения с результатом и критериями, owner только из списка участников (по роли и опыту), порядок и зависимости учти; если итог не планировочный — work пустой. Не выдумывай то, чего не обсуждали.\n\n";
    } else {
        s += &format!(
            "## Как участвовать (раунд {round} из {})\n\
- Говори от своей роли и опыта. При необходимости обратись к памяти (`senate_recall`) и к материалам проекта — но ничего не меняй: форум только для обсуждения.\n\
- Не повторяй уже сказанное. Добавляй новое: риск, альтернативу, уточнение, зависимость, оценку. Несогласие — нормально: назови основание.\n\
- Коротко: до 180 слов, без вступлений.\n\
- Если добавить нечего — ответь одним словом: ПАС.\n\n",
            meta["rounds"].as_i64().unwrap_or(2)
        );
    }
    s += &format!("## Обсуждение до сих пор\n{}", if transcript.is_empty() { "(пока никто не высказался)\n".to_string() } else { transcript });
    Ok(s)
}

/// What the environment already knows about the companies and projects the person just named, other than
/// the ones this chat is attached to (those are in the system prompt). A specialist is global: it works
/// across companies, and reads a company's memory when it is named — never by accident.
fn mention_block(store: &Store, agent: &Agent, thread: &Thread, text: &str) -> Result<String> {
    let attached_company = thread.company_id.clone().or(match &thread.project_id {
        Some(p) => store.company_of_project(p)?,
        None => None,
    });
    let mut s = String::new();
    for m in store.mentions(text)? {
        let is_attached = (m.kind == "project" && thread.project_id.as_deref() == Some(&m.id)) || (m.kind == "company" && attached_company.as_deref() == Some(&m.id));
        if is_attached {
            continue;
        }
        let (project, company) = if m.kind == "project" { (Some(m.id.as_str()), m.company_id.as_deref()) } else { (None, Some(m.id.as_str())) };
        let mem = store.search_memory_in(text, Some(&agent.id), project, company, 6)?;
        s += &format!("- {} «{}»", if m.kind == "project" { "Проект" } else { "Компания" }, m.name);
        if m.kind == "company" {
            if let Ok(c) = store.get_company(&m.id) {
                if !c.brief.trim().is_empty() {
                    s += &format!(": {}", clip(&c.brief, 400));
                }
            }
        } else if let Ok(p) = store.get_project(&m.id) {
            if !p.goal.trim().is_empty() {
                s += &format!(": {}", clip(&p.goal, 400));
            }
        }
        s += "\n";
        for e in mem {
            s += &format!("    · {}\n", clip(&e.body, 280));
        }
    }
    if s.is_empty() {
        return Ok(s);
    }
    Ok(format!("## Упомянуто в сообщении (найдено средой)\n{s}\n"))
}

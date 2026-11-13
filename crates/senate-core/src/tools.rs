//! Tools an agent can call to write into the environment. Transport-agnostic: the node exposes
//! them over MCP, tests call them directly. Every call is authorized by the environment
//! (Senate.md section 18) — a profile, a memory entry or a colleague's message cannot widen access.

use crate::engine::TransitionArgs;
use crate::error::{Error, Result};
use crate::model::{action, *};
use crate::store::Store;
use serde_json::{json, Value};

#[derive(Debug, Clone, Default)]
pub struct ToolCtx {
    /// The node's default model: a called colleague without a model of their own runs on it.
    pub default_backend: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub obligation_id: Option<String>,
    pub thread_id: Option<String>,
    pub project_id: Option<String>,
    pub company_id: Option<String>,
    /// The run is on the node (a server), not on the person's computer: a `localhost` it starts is the server's,
    /// and the person reaches it through the node (`/preview/<port>/`), not on their own machine.
    pub on_node: bool,
}

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

pub fn definitions() -> Vec<ToolDef> {
    let mem_kinds = json!(["observation", "hypothesis", "verified", "preference", "decision", "error"]);
    vec![
        ToolDef {
            name: "senate_recall",
            description: "Поиск в памяти Senate: личный опыт, знания проекта, проверенные общие знания. Исправленные записи не возвращаются.",
            schema: json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"},"company":{"type":"string","description":"название компании, если ищешь в её памяти"},"project":{"type":"string","description":"название проекта, если ищешь в его памяти"}},"required":["query"]}),
        },
        ToolDef {
            name: "senate_find",
            description: "Найти в среде компании, проекты и их знания по названию или словам. Когда человек называет клиента, проект или тему — сначала найди их здесь, а не спрашивай человека, где искать.",
            schema: json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}),
        },
        ToolDef {
            name: "senate_attach",
            description: "Привязать этот разговор к проекту или компании по названию (регистр не важен, хватит части названия, если она однозначна), чтобы их память, решения и папка стали частью контекста. Так выполняют просьбы «прикрепи чат к …», «перенеси в проект …». С одной компанией чат сохраняет свой проект, если тот не принадлежит другой компании. Делай это сам, когда ясно, о чём речь.",
            schema: json!({"type":"object","properties":{"project":{"type":"string"},"company":{"type":"string"}}}),
        },
        ToolDef {
            name: "senate_remember",
            description: "Сохранить знание. Укажи вид честно: наблюдение, гипотеза или проверенное (проверенное требует verification — основание проверки). Выбери слой памяти: личное мастерство, знание о компании (только моё или общее для компании) или знание проекта.",
            schema: json!({"type":"object","properties":{
                "body":{"type":"string"},"kind":{"type":"string","enum":mem_kinds},"level":{"type":"string","enum":["personal","company_personal","company","project"],"description":"personal — моё общее мастерство, идёт со мной во все компании; company_personal — что я знаю об этой компании, только мне; company — знание для всех, кто работает в компании; project — для проекта"},
                "source":{"type":"string","description":"откуда известно: файл, ссылка, id сессии"},
                "scope":{"type":"string","description":"где применимо"},"verification":{"type":"string"}},
                "required":["body","kind"]}),
        },
        ToolDef {
            name: "senate_correct_memory",
            description: "Исправить запись памяти: старая перестаёт использоваться, новая связывается с ней.",
            schema: json!({"type":"object","properties":{"id":{"type":"string"},"new_body":{"type":"string"},"source":{"type":"string"}},"required":["id","new_body"]}),
        },
        ToolDef {
            name: "senate_obligations",
            description: "Открытые задачи: свои (mine), проекта этого разговора (project) или все в Senate (all — если человек дал тебе право управлять чужими задачами). У каждой: id, название, состояние, исполнитель (slug), проект.",
            schema: json!({"type":"object","properties":{"scope":{"type":"string","enum":["mine","project","all"]}}}),
        },
        ToolDef {
            name: "senate_obligation_create",
            description: "Принять на себя поручение или поставить его коллеге (owner = slug коллеги). Нужны ожидаемый результат и критерии завершения.",
            schema: json!({"type":"object","properties":{"title":{"type":"string"},"result":{"type":"string"},"criteria":{"type":"string"},
                "owner":{"type":"string","description":"slug коллеги; по умолчанию — ты сам"},"next_step":{"type":"string"},"due_at":{"type":"integer","description":"unix мс"}},
                "required":["title","result"]}),
        },
        ToolDef {
            name: "senate_obligation_update",
            description: "Изменить состояние задачи. Свою — всегда; чужую или без исполнителя — если человек дал тебе такое право (иначе придёт pending_approval): так закрывают и отменяют задачи по просьбе человека, в любом проекте; пачку — через ids. done требует outcome (что получено), cancelled — outcome (причина отмены); blocked — blocker; waiting — wait (time | event | obligation | user). id по умолчанию — задача текущей сессии.",
            schema: json!({"type":"object","properties":{"id":{"type":"string"},
                "ids":{"type":"array","items":{"type":"string"},"description":"несколько задач сразу с одним и тем же состоянием и причиной/результатом: так закрывают пачку; ответ — по каждой"},
                "state":{"type":"string","enum":["running","waiting","blocked","review","done","cancelled"]},
                "outcome":{"type":"string"},"next_step":{"type":"string"},"blocker":{"type":"string"},
                "wait":{"type":"object","description":"{type:'time',at:<мс>} | {type:'event',kind,subject?} | {type:'obligation',id} | {type:'user'}"}},
                "required":["state"]}),
        },
        ToolDef {
            name: "senate_obligation_history",
            description: "История задачи: кто и когда её создал, менял, передавал, что записал. Чтобы проверить, действительно ли она сделана, прежде чем закрывать.",
            schema: json!({"type":"object","properties":{"id":{"type":"string"},"limit":{"type":"integer"}},"required":["id"]}),
        },
        ToolDef {
            name: "senate_obligation_reassign",
            description: "Передать задачу другому специалисту (owner = slug) или назначить исполнителя задаче без исполнителя. Чужую — если человек дал тебе право управлять чужими задачами.",
            schema: json!({"type":"object","properties":{"id":{"type":"string"},"owner":{"type":"string","description":"slug специалиста"}},"required":["id","owner"]}),
        },
        ToolDef {
            name: "senate_decision",
            description: "Предложить решение проекта. Статус «принято» фиксируется отдельно ответственным или пользователем.",
            schema: json!({"type":"object","properties":{"title":{"type":"string"},"body":{"type":"string"},"supersedes":{"type":"string"},"scope":{"type":"string","enum":["project","company"],"description":"company — общее решение для всех проектов компании"}},"required":["title"]}),
        },
        ToolDef {
            name: "senate_artifact_save",
            description: "Сохранить артефакт проекта (документ, исследование, заметки). Для файлов кода используй обычные инструменты редактирования.",
            schema: json!({"type":"object","properties":{"title":{"type":"string"},"kind":{"type":"string"},"body":{"type":"string"},"id":{"type":"string"}},"required":["title","body"]}),
        },
        ToolDef {
            name: "senate_project_create",
            description: "Предложить завести проект (и, если нужно, компанию) под эту работу и перенести в него этот разговор. Всегда требует согласия пользователя — сначала спроси его в чате, потом вызывай.",
            schema: json!({"type":"object","properties":{"name":{"type":"string"},"goal":{"type":"string"},"company":{"type":"string","description":"название компании; будет создана, если её нет"}},"required":["name"]}),
        },
        ToolDef {
            name: "senate_mcp_install",
            description: "Предложить установить MCP-сервер себе (нужен для задачи: браузер, документация, дизайн-инструмент и т.п.). Всегда требует согласия пользователя. Секреты (токены) ты не вводишь — укажи только их имена в env_keys, пользователь добавит значения сам. Сервер заработает со следующего запуска.",
            schema: json!({"type":"object","properties":{"name":{"type":"string","description":"короткое имя: латиница, цифры, - и _"},"kind":{"type":"string","enum":["stdio","http"]},
                "command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"url":{"type":"string"},
                "env_keys":{"type":"array","items":{"type":"string"},"description":"имена переменных, для которых нужны секреты"},
                "note":{"type":"string","description":"что это и зачем"},"reason":{"type":"string","description":"зачем он нужен именно сейчас"}},"required":["name","kind","note"]}),
        },
        ToolDef {
            name: "senate_api_connect",
            description: "Предложить подключить внешний API себе. Всегда требует согласия пользователя. Ключ ты не вводишь — пользователь добавит его сам. Запись в сервис по умолчанию потребует согласия на каждый вызов.",
            schema: json!({"type":"object","properties":{"name":{"type":"string"},"label":{"type":"string"},"base_url":{"type":"string"},
                "auth_type":{"type":"string","enum":["none","bearer","header","basic","query"]},"auth_name":{"type":"string"},
                "docs":{"type":"string","description":"что умеет API и как его вызывать"},"reason":{"type":"string"}},"required":["name","base_url","docs"]}),
        },
        ToolDef {
            name: "senate_api_list",
            description: "Подключённые к тебе внешние API: имя, назначение, адрес и правила записи. Ключи недоступны — их подставляет нода.",
            schema: json!({"type":"object","properties":{}}),
        },
        ToolDef {
            name: "senate_api_call",
            description: "Вызвать внешний API через ноду: service — имя из senate_api_list, method, path (начинается с /), query, body. Чтение (GET) идёт сразу; запись (POST/PUT/PATCH/DELETE) может потребовать согласия пользователя.",
            schema: json!({"type":"object","properties":{"service":{"type":"string"},"method":{"type":"string","enum":["GET","POST","PUT","PATCH","DELETE"]},
                "path":{"type":"string"},"query":{"type":"object"},"body":{}},"required":["service","path"]}),
        },
        ToolDef {
            name: "senate_colleagues",
            description: "Коллеги: кто есть в среде, роли и нагрузка.",
            schema: json!({"type":"object","properties":{}}),
        },
        ToolDef {
            name: "senate_call_colleague",
            description: "Позвать коллегу в этот разговор: задать вопрос или поручить небольшую часть работы (SEO, дизайн, рассылки, код…). Он ответит здесь же, а его ответ вернётся к тебе отдельным ходом — свою работу можешь продолжать или закончить ход. to — slug из senate_colleagues, request — что нужно и зачем, с контекстом: коллега не видит твоих мыслей и файлов, только то, что ты написал. Для долгой работы с результатом ставь задачу (senate_obligation_create), а не зови.",
            schema: json!({"type":"object","properties":{"to":{"type":"string","description":"slug коллеги"},"request":{"type":"string"}},"required":["to","request"]}),
        },
        ToolDef {
            name: "senate_message_colleague",
            description: "Записка коллеге — предупреждение, совет, факт, который ему важен прямо сейчас (например: «не запускай npm run build, пока идёт tauri dev»). Если он сейчас работает, увидит её с ближайшим ответом любого инструмента Senate; если свободен — записка сразу запускает его ход здесь (просьбу в ней он выполнит и отчитается тебе отдельным ходом). Ответ не ждёт; в результате видно, что произошло: delivered_now, delivered_started_turn или delivered_next_turn (ход не запущен — тогда скажи человеку). Проверить, прочитал ли, — senate_inbox. urgent=true для того, что может что-то сломать.",
            schema: json!({"type":"object","properties":{"to":{"type":"string","description":"slug коллеги"},"text":{"type":"string"},"urgent":{"type":"boolean"}},"required":["to","text"]}),
        },
        ToolDef {
            name: "senate_inbox",
            description: "Записки от коллег, которых ты ещё не видел, и судьба твоих записок (прочитана или ждёт). Во время долгой работы загляни сюда между крупными шагами: коллеги могут предупреждать о том, что мешает.",
            schema: json!({"type":"object","properties":{}}),
        },
        ToolDef {
            name: "senate_ask_colleague",
            description: "Спросить коллегу и подождать ответа, не заканчивая свой ход: короткий вопрос по его части. Ждёт не дольше wait_seconds (по умолчанию 120, максимум 300). Не ответил вовремя (занят, выбит лимит, упал) — придёт статус и причина, а ответ, если он появится, вернётся к тебе отдельным ходом: продолжай по своему решению. Для долгой работы — senate_call_colleague или задача.",
            schema: json!({"type":"object","properties":{"to":{"type":"string","description":"slug коллеги"},"question":{"type":"string"},"wait_seconds":{"type":"integer"}},"required":["to","question"]}),
        },
        ToolDef {
            name: "senate_ask_person",
            description: "Спросить человека посреди работы, не заканчивая ход: вопрос появится в чате с полем для ответа (и вариантами, если дашь options). Ждёт не дольше wait_seconds (по умолчанию 900, максимум 1800). Не ответил — продолжай по самому разумному варианту и скажи в итоге, что выбрал сам. Спрашивай только то, что нельзя решить самому.",
            schema: json!({"type":"object","properties":{"question":{"type":"string"},"options":{"type":"array","items":{"type":"string"}},"wait_seconds":{"type":"integer"}},"required":["question"]}),
        },
        ToolDef {
            name: "senate_image_generate",
            description: "Сделать картинку по описанию: иллюстрацию, фон, заглушку, баннер. Опиши, что на ней, в каком стиле и для чего — коротким предложением на русском или английском, как принято у сервисов. Картинка сохраняется на этом компьютере и сразу показывается человеку в чате; в ответе придёт путь к файлу (path) — копируй её в папку проекта, если она нужна в работе. service — имя подключения из senate_api_list (если оно одно для картинок, не указывай), model — модель сервиса, size — размер вида 1024x1024. Ключи сервиса у ноды: тебе они не нужны и не выдаются.",
            schema: json!({"type":"object","properties":{"prompt":{"type":"string"},"size":{"type":"string"},"model":{"type":"string"},"service":{"type":"string"}},"required":["prompt"]}),
        },
        ToolDef {
            name: "senate_access_list",
            description: "Доступы, которые есть у человека: серверы, базы, сайты. Название, вид, назначение и выдан ли тебе. Самих паролей и ключей здесь нет — их выдаёт senate_access_get.",
            schema: json!({"type":"object","properties":{}}),
        },
        ToolDef {
            name: "senate_access_get",
            description: "Получить доступ (адрес, логин, пароль или ключ) к серверу, базе или сайту, чтобы выполнить работу. Выдаётся только если человек разрешил тебе этот доступ. Если нужно его согласие каждый раз, придёт pending_approval: скажи человеку, что ждёшь, и после согласия повтори вызов — доступ выдастся один раз. reason — зачем он нужен. Пароль и ключ используй только в командах и подключениях: не пиши их в ответ, память, коммиты и файлы проекта.",
            schema: json!({"type":"object","properties":{"name":{"type":"string","description":"название из senate_access_list"},"reason":{"type":"string","description":"зачем нужен доступ"}},"required":["name"]}),
        },
        ToolDef {
            name: "senate_preview",
            description: "Показать человеку кнопку со ссылкой на локальный сервер разработки (вместо подсказки под полем ввода). Вызывай, когда запустил сервер или страницу, которую человеку надо посмотреть: url вида http://localhost:3000/путь. Разрешены только адреса этого компьютера (localhost, 127.0.0.1, [::1]). Остановил сервер или закончил — вызови с clear=true, чтобы убрать кнопку.",
            schema: json!({"type":"object","properties":{"url":{"type":"string","description":"http://localhost:PORT[/путь]"},"label":{"type":"string","description":"подпись на кнопке (по умолчанию адрес)"},"clear":{"type":"boolean","description":"убрать кнопку"}}}),
        },
    ]
}

/// A project by the name a person typed: exact in any case, else the only one that contains it.
fn project_by_name(store: &Store, name: &str) -> Result<Option<Project>> {
    let want = crate::store::fold_name(name);
    let all = store.list_projects()?;
    if let Some(p) = all.iter().find(|p| crate::store::fold_name(&p.name) == want) {
        return Ok(Some(p.clone()));
    }
    let partial: Vec<&Project> = all.iter().filter(|p| crate::store::fold_name(&p.name).contains(&want)).collect();
    Ok(if partial.len() == 1 { Some(partial[0].clone()) } else { None })
}

/// A link the person's own machine can open safely: http(s) to the loopback interface only.
/// The agent chooses the URL, so anything that could lead off this machine is refused.
/// Returns the normalised URL and a short default label (`localhost:3000`).
pub fn local_preview_url(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 300 || raw.chars().any(|c| c.is_control() || c.is_whitespace() || c == '\\') {
        return None;
    }
    // Agents often write a bare `localhost:3006`: that is the same address.
    let lower = raw.to_lowercase();
    let bare = !lower.starts_with("http://") && !lower.starts_with("https://") && !lower.contains("://");
    let owned;
    let raw = if bare {
        owned = format!("http://{raw}");
        owned.as_str()
    } else {
        raw
    };
    let rest = raw.strip_prefix("http://").or_else(|| raw.strip_prefix("https://"))?;
    let authority_end = rest.find(|c| c == '/' || c == '?' || c == '#').unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    // userinfo (`localhost@evil.com`) is how a link pretends to be local
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, after) = v6.split_once(']')?;
        (format!("[{h}]"), after.strip_prefix(':').map(str::to_string).or(if after.is_empty() { None } else { Some(after.to_string()) }))
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p.to_string())),
            None => (authority.to_string(), None),
        }
    };
    let h = host.to_lowercase();
    let local = h == "localhost" || h.ends_with(".localhost") || h == "127.0.0.1" || h == "[::1]";
    if !local {
        return None;
    }
    if let Some(p) = &port {
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()) || p.parse::<u32>().ok().filter(|n| (1..=65535).contains(n)).is_none() {
            return None;
        }
    }
    let scheme = if raw.starts_with("https://") { "https" } else { "http" };
    let label = match &port {
        Some(p) => format!("{h}:{p}"),
        None => h.clone(),
    };
    Some((format!("{scheme}://{authority}{tail}"), label))
}

fn s<'a>(a: &'a Value, k: &str) -> &'a str {
    a.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

fn pending(store: &Store, ctx: &ToolCtx, act: &str, name: &str, args: &Value) -> Result<Value> {
    let detail = json!({"tool": name, "args": args, "ctx": {
        "agent_id": ctx.agent_id, "session_id": ctx.session_id, "obligation_id": ctx.obligation_id,
        "thread_id": ctx.thread_id, "project_id": ctx.project_id, "company_id": ctx.company_id}});
    let ap = store.create_approval(&ctx.agent_id, ctx.session_id.as_deref(), ctx.obligation_id.as_deref(), act, &detail)?;
    Ok(json!({"status": "pending_approval", "approval_id": ap.id, "message": "Действие ждёт согласования пользователя."}))
}

fn gate(store: &Store, ctx: &ToolCtx, act: &str, name: &str, args: &Value, bypass: bool) -> Result<Option<Value>> {
    if bypass {
        return Ok(None);
    }
    match store.authorize(&ctx.agent_id, ctx.project_id.as_deref(), act)? {
        GrantMode::Auto => Ok(None),
        GrantMode::Approve => Ok(Some(pending(store, ctx, act, name, args)?)),
        GrantMode::Deny => Err(Error::Forbidden(format!("действие {act} запрещено для этого специалиста"))),
    }
}

pub fn call(store: &Store, ctx: &ToolCtx, name: &str, args: &Value) -> Result<Value> {
    call_inner(store, ctx, name, args, false)
}

/// Run an action the user has just approved.
pub fn execute_approved(store: &Store, approval: &Approval) -> Result<Value> {
    let tool = approval.detail["tool"].as_str().unwrap_or_default().to_string();
    let c = &approval.detail["ctx"];
    let ctx = ToolCtx {
        agent_id: c["agent_id"].as_str().unwrap_or(&approval.agent_id).to_string(),
        session_id: c["session_id"].as_str().map(String::from),
        obligation_id: c["obligation_id"].as_str().map(String::from),
        thread_id: c["thread_id"].as_str().map(String::from),
        project_id: c["project_id"].as_str().map(String::from),
        company_id: c["company_id"].as_str().map(String::from),
        ..Default::default()
    };
    call_inner(store, &ctx, &tool, &approval.detail["args"], true)
}

fn call_inner(store: &Store, ctx: &ToolCtx, name: &str, a: &Value, bypass: bool) -> Result<Value> {
    match name {
        "senate_recall" => {
            let limit = a.get("limit").and_then(|v| v.as_i64()).unwrap_or(8).clamp(1, 20);
            let (mut project_id, mut company_id) = scope_of(store, ctx);
            if !s(a, "project").is_empty() {
                if let Some(p) = project_by_name(store, s(a, "project"))? {
                    company_id = p.company_id.clone();
                    project_id = Some(p.id);
                }
            } else if !s(a, "company").is_empty() {
                if let Some(c) = store.company_like(s(a, "company"))? {
                    company_id = Some(c.id);
                    project_id = None;
                }
            }
            let mem = store.search_memory_in(s(a, "query"), Some(&ctx.agent_id), project_id.as_deref(), company_id.as_deref(), limit)?;
            Ok(json!(mem.iter().map(|m| json!({
                "id": m.id, "level": m.level.as_str(), "kind": m.kind.as_str(), "body": m.body,
                "source": m.source, "scope": m.scope_note, "verification": m.verification,
                "company": m.company_id
            })).collect::<Vec<_>>()))
        }
        "senate_find" => {
            let q = s(a, "query");
            let found = store.mentions(q)?;
            let mut out = vec![];
            for m in &found {
                let (p, c) = if m.kind == "project" { (Some(m.id.as_str()), m.company_id.as_deref()) } else { (None, Some(m.id.as_str())) };
                let mem = store.search_memory_in(q, Some(&ctx.agent_id), p, c, 6)?;
                let detail = if m.kind == "project" { store.get_project(&m.id).map(|x| x.goal).unwrap_or_default() } else { store.get_company(&m.id).map(|x| x.brief).unwrap_or_default() };
                out.push(json!({"kind": m.kind, "name": m.name, "id": m.id, "about": detail.chars().take(500).collect::<String>(),
                    "knowledge": mem.iter().map(|e| json!({"kind": e.kind.as_str(), "body": e.body.chars().take(400).collect::<String>()})).collect::<Vec<_>>()}));
            }
            // free-text search across everything this specialist may see, in case nothing was named exactly
            let loose = store.search_memory_in(q, Some(&ctx.agent_id), None, None, 5)?;
            Ok(json!({"matches": out, "general_memory": loose.iter().map(|e| e.body.chars().take(300).collect::<String>()).collect::<Vec<_>>()}))
        }
        "senate_attach" => {
            let tid = ctx.thread_id.clone().ok_or_else(|| Error::invalid("у этого запуска нет разговора"))?;
            let project = if s(a, "project").is_empty() { None } else { project_by_name(store, s(a, "project"))? };
            let company = if s(a, "company").is_empty() { None } else { store.company_like(s(a, "company"))? };
            if !s(a, "project").is_empty() && project.is_none() {
                return Err(Error::not_found(format!("проект «{}»; названия проектов — в senate_find", s(a, "project"))));
            }
            if !s(a, "company").is_empty() && company.is_none() {
                let names: Vec<String> = store.list_companies()?.into_iter().map(|c| c.name).collect();
                return Err(Error::not_found(format!("компания «{}»; есть: {}", s(a, "company"), names.join(", "))));
            }
            let (p, c) = match (&project, &company) {
                (Some(p), _) => (Some(p.id.clone()), p.company_id.clone().or_else(|| company.as_ref().map(|c| c.id.clone()))),
                (None, Some(c)) => {
                    // Filing a chat under a company keeps its project, unless that project belongs to another company.
                    let cur = store.get_thread(&tid)?.project_id;
                    let keep = match &cur {
                        Some(pid) => store.get_project(pid).ok().filter(|p| p.company_id.as_deref().map_or(true, |x| x == c.id)).map(|p| p.id),
                        None => None,
                    };
                    (keep, Some(c.id.clone()))
                }
                _ => return Err(Error::invalid("укажи project или company")),
            };
            store.move_thread(&tid, p.as_deref(), c.as_deref())?;
            let company_name = match &c { Some(id) => store.list_companies()?.into_iter().find(|x| &x.id == id).map(|x| x.name), None => None };
            Ok(json!({"status": "attached", "project": project.map(|x| x.name), "kept_project": p, "company": company_name}))
        }
        "senate_remember" => {
            if let Some(p) = gate(store, ctx, action::MEMORY_WRITE, name, a, bypass)? {
                return Ok(p);
            }
            // Layers: personal = my general craft, follows me everywhere; company_personal = what I know about
            // this company, only mine; company = shared by everyone working in this company; project = this project.
            let layer = s(a, "level");
            let (level, company_id, project_id) = match layer {
                "project" => {
                    if ctx.project_id.is_none() {
                        return Err(Error::invalid("проектная память требует проект; используй level=personal"));
                    }
                    (MemoryLevel::Project, None, ctx.project_id.clone())
                }
                "company" | "company_personal" => {
                    let Some(co) = ctx.company_id.clone() else {
                        return Err(Error::invalid("этот разговор не относится к компании; используй level=personal"));
                    };
                    (if layer == "company" { MemoryLevel::Company } else { MemoryLevel::Personal }, Some(co), None)
                }
                _ => (MemoryLevel::Personal, None, None),
            };
            let kind = MemoryKind::parse(s(a, "kind")).ok_or_else(|| Error::invalid("неизвестный вид знания"))?;
            let src = if s(a, "source").is_empty() { ctx.session_id.clone().unwrap_or_default() } else { s(a, "source").to_string() };
            let m = store.add_memory(NewMemory {
                level,
                agent_id: if level == MemoryLevel::Company { None } else { Some(ctx.agent_id.clone()) },
                project_id,
                company_id,
                kind,
                body: s(a, "body").into(),
                source: src,
                scope_note: s(a, "scope").into(),
                verification: s(a, "verification").into(),
                fresh_until: None,
            })?;
            Ok(json!({"id": m.id, "status": "saved"}))
        }
        "senate_correct_memory" => {
            if let Some(p) = gate(store, ctx, action::MEMORY_WRITE, name, a, bypass)? {
                return Ok(p);
            }
            let old = store.get_memory(s(a, "id"))?;
            // Personal entries are the owner's; shared ones may be corrected by anyone in scope.
            if old.level == MemoryLevel::Personal && old.agent_id.as_deref() != Some(&ctx.agent_id) {
                return Err(Error::Forbidden("чужая личная память".into()));
            }
            let m = store.supersede_memory(&old.id, s(a, "new_body"), s(a, "source"), &ctx.agent_id)?;
            Ok(json!({"id": m.id, "replaced": old.id}))
        }
        "senate_obligations" => {
            let (project_id, _) = scope_of(store, ctx);
            let list = match s(a, "scope") {
                "all" => {
                    // the whole picture is for whoever the person lets manage tasks (asking each time counts)
                    if store.authorize(&ctx.agent_id, ctx.project_id.as_deref(), action::OBLIGATION_MANAGE)? == GrantMode::Deny {
                        return Err(Error::Forbidden("видеть все задачи может тот, кому человек разрешил управлять чужими задачами".into()));
                    }
                    store.list_obligations(None, None, true)?
                }
                "project" if project_id.is_some() => store.list_obligations(None, project_id.as_deref(), true)?,
                _ => store.list_obligations(Some(&ctx.agent_id), None, true)?,
            };
            let slugs: std::collections::HashMap<String, String> = store.list_agents(true)?.into_iter().map(|x| (x.id, x.slug)).collect();
            let projects: std::collections::HashMap<String, String> = store.list_projects()?.into_iter().map(|p| (p.id, p.name)).collect();
            Ok(json!(list.iter().map(|o| json!({
                "id": o.id, "title": o.title, "state": o.state.as_str(),
                "owner": o.owner_id.as_ref().and_then(|i| slugs.get(i)).cloned().unwrap_or_default(),
                "project": o.project_id.as_ref().and_then(|i| projects.get(i)).cloned().unwrap_or_default(),
                "next_step": o.next_step, "result": o.result, "criteria": o.criteria,
                // why it stands still, right in the list: no digging through the history
                "blocker": if o.state == ObligationState::Blocked { json!(o.blocker) } else { Value::Null },
                "waiting_for": o.wait_condition.as_ref().map(|w| json!(w)).unwrap_or(Value::Null),
            })).collect::<Vec<_>>()))
        }
        "senate_obligation_create" => {
            let owner = if s(a, "owner").is_empty() { ctx.agent_id.clone() } else { store.get_agent(s(a, "owner"))?.id };
            let act = if owner == ctx.agent_id { action::OBLIGATION_CREATE } else { action::OBLIGATION_DELEGATE };
            if let Some(p) = gate(store, ctx, act, name, a, bypass)? {
                return Ok(p);
            }
            let o = store.create_obligation(NewObligation {
                title: s(a, "title").into(),
                result: s(a, "result").into(),
                criteria: s(a, "criteria").into(),
                owner_id: Some(owner.clone()),
                project_id: ctx.project_id.clone(),
                parent_id: ctx.obligation_id.clone(),
                requested_by: ctx.agent_id.clone(),
                next_step: s(a, "next_step").into(),
                due_at: a.get("due_at").and_then(|v| v.as_i64()),
                budget_tokens: 0,
            })?;
            Ok(json!({"id": o.id, "owner": owner, "state": o.state.as_str()}))
        }
        "senate_obligation_update" => {
            if let Some(p) = gate(store, ctx, action::OBLIGATION_UPDATE, name, a, bypass)? {
                return Ok(p);
            }
            // A batch: the same change for each, one answer per task (a failure of one does not stop the rest).
            if let Some(ids) = a.get("ids").and_then(|v| v.as_array()) {
                let mut results = vec![];
                for id in ids.iter().filter_map(|v| v.as_str()).take(100) {
                    let mut one = a.clone();
                    one["ids"] = Value::Null;
                    one["id"] = json!(id);
                    results.push(match call_inner(store, ctx, name, &one, bypass) {
                        Ok(v) => json!({"id": id, "result": v}),
                        Err(e) => json!({"id": id, "error": e.to_string()}),
                    });
                }
                return Ok(json!({"results": results}));
            }
            let id = if s(a, "id").is_empty() { ctx.obligation_id.clone().ok_or_else(|| Error::invalid("укажите id задача"))? } else { s(a, "id").to_string() };
            let cur = store.get_obligation(&id)?;
            let own = cur.owner_id.as_deref() == Some(&ctx.agent_id);
            if !own {
                if let Some(p) = gate(store, ctx, action::OBLIGATION_MANAGE, name, a, bypass)? {
                    return Ok(p);
                }
            }
            let mut to = ObligationState::parse(s(a, "state")).ok_or_else(|| Error::invalid("неизвестное состояние"))?;
            // "Done" is a claim. With criteria on the table somebody other than the author checks it.
            let mut note = String::new();
            if own && to == ObligationState::Done && !cur.criteria.trim().is_empty() && cur.state != ObligationState::Review {
                to = ObligationState::Review;
                note = "готово по заявлению исполнителя; нужна проверка по критериям".into();
            }
            let wait = match a.get("wait") {
                Some(w) if !w.is_null() => Some(serde_json::from_value::<WaitCondition>(w.clone()).map_err(|e| Error::invalid(format!("условие ожидания: {e}")))?),
                _ => None,
            };
            // Running → running with a new next step is a plain progress note.
            if cur.state == to && to == ObligationState::Running {
                store.update_obligation_fields(&id, None, None, None, a.get("next_step").and_then(|v| v.as_str()), None, &ctx.agent_id)?;
                return Ok(json!({"id": id, "state": "running"}));
            }
            let o = store.transition(
                &id,
                to,
                TransitionArgs {
                    wait,
                    blocker: Some(s(a, "blocker").into()),
                    outcome: if s(a, "outcome").is_empty() { None } else { Some(s(a, "outcome").into()) },
                    next_step: if s(a, "next_step").is_empty() { None } else { Some(s(a, "next_step").into()) },
                    note,
                },
                &ctx.agent_id,
            )?;
            Ok(json!({"id": o.id, "state": o.state.as_str()}))
        }
        "senate_obligation_history" => {
            let o = store.get_obligation(s(a, "id"))?;
            let limit = a.get("limit").and_then(|v| v.as_i64()).unwrap_or(30).clamp(1, 100);
            let events = store.events(Some("obligation"), Some(&o.id), limit)?;
            Ok(json!({
                "id": o.id, "title": o.title, "state": o.state.as_str(), "owner": o.owner_id, "result": o.result, "criteria": o.criteria,
                "outcome": o.outcome, "next_step": o.next_step,
                "events": events.iter().map(|e| json!({"at": e.ts, "kind": e.kind, "actor": e.actor, "data": e.payload})).collect::<Vec<_>>()
            }))
        }
        "senate_obligation_reassign" => {
            let cur = store.get_obligation(s(a, "id"))?;
            let new_owner = store.get_agent(s(a, "owner"))?;
            let own = cur.owner_id.as_deref() == Some(&ctx.agent_id);
            if own {
                if let Some(p) = gate(store, ctx, action::OBLIGATION_DELEGATE, name, a, bypass)? {
                    return Ok(p);
                }
            } else {
                if let Some(p) = gate(store, ctx, action::OBLIGATION_MANAGE, name, a, bypass)? {
                    return Ok(p);
                }
            }
            let o = store.reassign_obligation(&cur.id, &new_owner.id, &ctx.agent_id)?;
            Ok(json!({"id": o.id, "owner": new_owner.slug}))
        }
        "senate_decision" => {
            if let Some(p) = gate(store, ctx, action::DECISION_PROPOSE, name, a, bypass)? {
                return Ok(p);
            }
            let sup = if s(a, "supersedes").is_empty() { None } else { Some(s(a, "supersedes")) };
            let d = if s(a, "scope") == "company" {
                let co = ctx.company_id.clone().ok_or_else(|| Error::invalid("этот разговор не относится к компании"))?;
                store.propose_decision_in(None, Some(&co), s(a, "title"), s(a, "body"), &ctx.agent_id, sup)?
            } else {
                let pid = ctx.project_id.clone().ok_or_else(|| Error::invalid("решение принимается в рамках проекта или компании"))?;
                store.propose_decision(&pid, s(a, "title"), s(a, "body"), &ctx.agent_id, sup)?
            };
            Ok(json!({"id": d.id, "status": "proposed"}))
        }
        "senate_artifact_save" => {
            if let Some(p) = gate(store, ctx, action::ARTIFACT_WRITE, name, a, bypass)? {
                return Ok(p);
            }
            let kind = if s(a, "kind").is_empty() { "document" } else { s(a, "kind") };
            let id = if s(a, "id").is_empty() { None } else { Some(s(a, "id")) };
            let art = store.save_artifact(id, ctx.project_id.as_deref(), kind, s(a, "title"), s(a, "body"), &ctx.agent_id)?;
            Ok(json!({"id": art.id, "version": art.version}))
        }
        "senate_project_create" => {
            if let Some(p) = gate(store, ctx, action::PROJECT_CREATE, name, a, bypass)? {
                return Ok(p);
            }
            let company = if s(a, "company").trim().is_empty() {
                ctx.company_id.clone()
            } else {
                match store.company_by_name(s(a, "company"))? {
                    Some(c) => Some(c.id),
                    None => Some(store.create_company(NewCompany { name: s(a, "company").to_string(), ..Default::default() })?.id),
                }
            };
            let members: Vec<String> = store.list_agents(false)?.into_iter().map(|x| x.id).collect();
            let p = store.create_project(NewProject { name: s(a, "name").to_string(), goal: s(a, "goal").to_string(), members, company_id: company.clone(), ..Default::default() })?;
            if let Some(t) = &ctx.thread_id {
                store.move_thread(t, Some(&p.id), company.as_deref())?;
            }
            Ok(json!({"project_id": p.id, "company_id": company, "status": "created", "note": "разговор перенесён в проект"}))
        }
        "senate_mcp_install" => {
            if let Some(p) = gate(store, ctx, action::MCP_INSTALL, name, a, bypass)? {
                return Ok(p);
            }
            let kind = if s(a, "kind") == "http" { "http" } else { "stdio" };
            let mut env = std::collections::BTreeMap::new();
            for k in a.get("env_keys").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                if let Some(k) = k.as_str() {
                    env.insert(k.to_string(), String::new());
                }
            }
            let m = store.save_mcp(
                None,
                McpInput {
                    name: Some(s(a, "name").to_string()),
                    kind: Some(kind.into()),
                    command: Some(s(a, "command").to_string()),
                    args: Some(a.get("args").and_then(|v| v.as_array()).map(|x| x.iter().filter_map(|y| y.as_str().map(String::from)).collect()).unwrap_or_default()),
                    url: Some(s(a, "url").to_string()),
                    env: Some(env),
                    note: Some(s(a, "note").to_string()),
                    // Installed for the specialist who asked, not for everyone; and not trusted: its tools ask.
                    agents: Some(vec![ctx.agent_id.clone()]),
                    ..Default::default()
                },
            )?;
            Ok(json!({"status": "installed", "server": m.name, "needs_secrets": m.view()["missing"], "note": "заработает со следующего запуска; пока нет значений секретов, сервер не подключится"}))
        }
        "senate_api_connect" => {
            if let Some(p) = gate(store, ctx, action::API_CONNECT, name, a, bypass)? {
                return Ok(p);
            }
            let it = store.save_integration(
                None,
                IntegrationInput {
                    name: Some(s(a, "name").to_string()),
                    label: Some(s(a, "label").to_string()),
                    base_url: Some(s(a, "base_url").to_string()),
                    auth_type: Some(if s(a, "auth_type").is_empty() { "bearer".into() } else { s(a, "auth_type").to_string() }),
                    auth_name: Some(s(a, "auth_name").to_string()),
                    docs: Some(s(a, "docs").to_string()),
                    agents: Some(vec![ctx.agent_id.clone()]),
                    ..Default::default()
                },
            )?;
            Ok(json!({"status": "connected", "service": it.name, "needs_secret": it.secret.is_empty() && it.auth_type != "none", "note": "ключ добавит пользователь во вкладке API"}))
        }
        "senate_api_list" => {
            let list = store.integrations_for_agent(&ctx.agent_id)?;
            Ok(json!(list.iter().map(|i| json!({"service": i.name, "label": i.label, "base_url": i.base_url, "docs": i.docs, "writes": i.write_mode})).collect::<Vec<_>>()))
        }
        // The HTTP call itself is made by the node (it holds the secrets); these arms are only reached
        // when a stored approval is replayed.
        "senate_api_call" | "senate_image_generate" => Ok(json!({"deferred": true})),
        "senate_colleagues" => {
            let agents = store.list_agents(false)?;
            let mut out = vec![];
            for ag in agents {
                let open = store.list_obligations(Some(&ag.id), None, true)?.len();
                let working = store.list_sessions(Some(&ag.id), None, Some(SessionStatus::Running), 20)?.len();
                out.push(json!({"slug": ag.slug, "name": ag.name, "role": ag.role, "open_obligations": open, "working_now": working, "you": ag.id == ctx.agent_id}));
            }
            Ok(json!(out))
        }
        "senate_call_colleague" => {
            if let Some(p) = gate(store, ctx, action::COLLEAGUE_CALL, name, a, bypass)? {
                return Ok(p);
            }
            let tid = ctx.thread_id.clone().ok_or_else(|| Error::invalid("позвать коллегу можно только в разговоре"))?;
            let caller = store.get_agent(&ctx.agent_id)?;
            let r = store.call_colleague(&caller, &tid, s(a, "to"), s(a, "request"), turn_depth(store, ctx), &ctx.default_backend)?;
            Ok(json!({"status": "called", "to": r["to"], "message_id": r["message_id"], "note": "коллега ответит в этом разговоре; ответ вернётся к тебе отдельным ходом"}))
        }
        "senate_message_colleague" => {
            let from = store.get_agent(&ctx.agent_id)?;
            let to = store.get_agent(s(a, "to")).map_err(|_| Error::invalid(format!("нет коллеги «{}»: посмотри slug в senate_colleagues", s(a, "to"))))?;
            if to.id == from.id {
                return Err(Error::invalid("записка самому себе не нужна"));
            }
            let text: String = s(a, "text").trim().chars().take(4000).collect();
            if text.is_empty() {
                return Err(Error::invalid("пустая записка"));
            }
            let urgent = a.get("urgent").and_then(|v| v.as_bool()).unwrap_or(false);
            let id = store.add_note(ctx.thread_id.as_deref(), &from.id, &to.id, &text, urgent)?;
            let busy = store.agent_running(&to.id)? || store.agent_queued(&to.id)?;
            // Free colleague: a note is not left lying until they happen to work — it wakes them, like a call (the
            // note itself comes in their turn's notes). The call-chain limit still holds, and says so plainly.
            if !busy {
                if let Some(tid) = ctx.thread_id.clone() {
                    // the note's text is the turn's task itself: it survives a failed turn and a retry
                    let request = format!("Записка от {} (@{}){}:\n{}\n\nЕсли в ней просьба — выполни её и коротко отчитайся; если это сведения — учти и ответь одной строкой.", from.name, from.slug, if urgent { ", срочно" } else { "" }, text);
                    match store.call_colleague(&from, &tid, &to.slug, &request, turn_depth(store, ctx), &ctx.default_backend) {
                        Ok(_) => {
                            let _ = store.mark_note_delivered(&id);
                            return Ok(json!({"status": "delivered_started_turn", "note_id": id, "note": format!("{} был свободен: записка запустила его ход в этом разговоре. Его ответ вернётся к тебе отдельным ходом.", to.name)}));
                        }
                        Err(e) => {
                            if let Some(t) = &ctx.thread_id {
                                let _ = store.post_message(t, "agent", Some(&from.id), &format!("Записка @{}: {}", to.slug, text), json!({"note": {"to": to.id, "id": id, "urgent": urgent}}), MessageStatus::Done, None);
                            }
                            return Ok(json!({"status": "delivered_next_turn", "note_id": id, "note": format!("Записка сохранена, но ход {} НЕ запущен ({e}). Он увидит её, только когда начнёт работать. Если нужно сейчас — скажи человеку.", to.name)}));
                        }
                    }
                }
            }
            // a visible trace in the chat, so the person sees what colleagues told each other
            if let Some(t) = &ctx.thread_id {
                let _ = store.post_message(t, "agent", Some(&from.id), &format!("Записка @{}: {}", to.slug, text), json!({"note": {"to": to.id, "id": id, "urgent": urgent}}), MessageStatus::Done, None);
            }
            Ok(json!({
                "status": if busy { "delivered_now" } else { "delivered_next_turn" },
                "note_id": id,
                "note": if busy { format!("{} сейчас работает (или его ход в очереди) и увидит записку с ближайшим ответом инструмента Senate или в начале хода. Прочитана ли — senate_inbox.", to.name) } else { format!("{} увидит записку в начале следующего хода.", to.name) },
            }))
        }
        "senate_inbox" => {
            let notes = store.take_notes(&ctx.agent_id)?;
            let sent = store.sent_notes(&ctx.agent_id, 10)?;
            Ok(json!({"new": notes, "sent": sent.iter().map(|n| json!({"to": n["to"], "text": n["text"], "read": !n["read_at"].is_null()})).collect::<Vec<_>>()}))
        }
        "senate_access_list" => {
            let list: Vec<Value> = store
                .list_accesses()?
                .into_iter()
                .filter(|x| x.enabled)
                .map(|x| {
                    let mode = x.mode_for(&ctx.agent_id).to_string();
                    json!({
                        "name": x.name, "label": x.label, "kind": x.kind, "note": x.note,
                        "yours": match mode.as_str() { "allow" => "выдан", "ask" => "выдаётся с согласия человека", _ => "не выдан" },
                        // where it is only the holder's business to know
                        "host": if mode == "none" { String::new() } else { x.host.clone() },
                    })
                })
                .collect();
            Ok(json!(list))
        }
        "senate_access_get" => {
            let acc = store.get_access(s(a, "name"))?;
            if !acc.enabled {
                return Err(Error::Forbidden("этот доступ отключён человеком".into()));
            }
            let why = s(a, "reason");
            let deliver = |how: &str| -> Result<Value> {
                store.note_access_use(&acc.id, &ctx.agent_id, why, how)?;
                Ok(json!({
                    "name": acc.name, "kind": acc.kind, "host": acc.host, "login": acc.login, "secret": acc.secret, "note": acc.note,
                    "warning": "Секрет — только для команд и подключений. Не пиши его в ответ, память, коммиты и файлы проекта."
                }))
            };
            match acc.mode_for(&ctx.agent_id) {
                "allow" => deliver("разрешён"),
                "ask" => {
                    // the person's "yes" to one use: it is turned into a permit, spent by the specialist's next call
                    if bypass {
                        store.add_access_permit(&acc.id, &ctx.agent_id)?;
                        return Ok(json!({"status": "permitted"}));
                    }
                    if store.take_access_permit(&acc.id, &ctx.agent_id)? {
                        return deliver("по согласию");
                    }
                    let mut v = pending(store, ctx, "access.use", name, a)?;
                    v["message"] = json!("Человек решает, давать ли тебе этот доступ. Скажи ему, что ждёшь, и повтори вызов после согласия: доступ выдастся один раз.");
                    Ok(v)
                }
                _ => {
                    store.note_access_use(&acc.id, &ctx.agent_id, why, "отказано")?;
                    Err(Error::Forbidden(format!("доступ «{}» тебе не выдан. Человек может выдать его в разделе «Доступы».", acc.name)))
                }
            }
        }
        // A button under the composer that opens the dev server. Local addresses only, so no approval is
        // needed: the worst an agent can do is point the button at a page on the person's own machine.
        "senate_preview" => {
            let tid = ctx.thread_id.clone().ok_or_else(|| Error::invalid("у этого запуска нет разговора"))?;
            if a.get("clear").and_then(|v| v.as_bool()).unwrap_or(false) || (s(a, "url").trim().is_empty()) {
                store.set_forum_meta(&tid, &json!({"preview": null}))?;
                return Ok(json!({"status": "cleared"}));
            }
            let (url, default_label) = local_preview_url(s(a, "url"))
                .ok_or_else(|| Error::invalid("разрешены только адреса этого компьютера: http://localhost:PORT, http://127.0.0.1:PORT или http://[::1]:PORT"))?;
            let label: String = s(a, "label").chars().filter(|c| !c.is_control()).take(40).collect();
            let label = if label.trim().is_empty() { default_label } else { label.trim().to_string() };
            // where the dev server runs decides how the person reaches it
            let on = if ctx.on_node { "node" } else { "machine" };
            store.set_forum_meta(&tid, &json!({"preview": {"url": url, "label": label, "on": on}}))?;
            let note = if ctx.on_node { "кнопка появилась под полем ввода; ты на сервере, человек откроет страницу через Senate (без доступа к его localhost)" } else { "кнопка появилась под полем ввода" };
            Ok(json!({"status": "set", "url": url, "note": note}))
        }
        other => Err(Error::invalid(format!("неизвестный инструмент {other}"))),
    }
}

/// How deep in a chain of calls this turn already is (a call, an answer, a call …).
fn turn_depth(store: &Store, ctx: &ToolCtx) -> i64 {
    ctx.session_id
        .as_deref()
        .and_then(|s| store.get_session(s).ok())
        .and_then(|s| s.message_id)
        .and_then(|m| store.get_message(&m).ok())
        .map(|m| m.props["depth"].as_i64().unwrap_or(0))
        .unwrap_or(0)
}

/// Where this run really is: the chat may have been attached after the run started.
fn scope_of(store: &Store, ctx: &ToolCtx) -> (Option<String>, Option<String>) {
    if let Some(t) = ctx.thread_id.as_deref().and_then(|t| store.get_thread(t).ok()) {
        let company = t.company_id.clone().or_else(|| t.project_id.as_deref().and_then(|p| store.company_of_project(p).ok().flatten()));
        return (t.project_id, company);
    }
    (ctx.project_id.clone(), ctx.company_id.clone())
}

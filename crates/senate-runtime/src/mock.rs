//! Demo backend: no model, no network. Used for first-run demos, UI development and tests. It
//! walks through the same events a real backend produces — steps, a permission question, an answer.

use crate::{Approver, CancelRx, Decision, Event, EventTx, Outcome, PermissionRequest, Progress, RunSpec, Step, Work};
use serde_json::json;
use std::time::Duration;

async fn pause(ms: u64, cancel: &mut CancelRx) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_millis(ms)) => false,
        _ = cancel.changed() => *cancel.borrow(),
    }
}

pub async fn run(spec: RunSpec, approver: &dyn Approver, tx: EventTx, mut cancel: CancelRx) -> Outcome {
    // Test hook: a CLI whose session is still held by another process refuses to resume it.
    if spec.resume.is_some() && spec.prompt.contains("СЕССИЯ-ЗАНЯТА") {
        return Outcome { failure: Some("thread/resume failed: thread abc already has an active writer (code -32600)".into()), ..Default::default() };
    }
    // Test hook: the CLI has forgotten the session altogether (Kimi's words for it).
    if spec.resume.is_some() && spec.prompt.contains("СЕССИЯ-ПОТЕРЯНА") {
        return Outcome { failure: Some("session not found: Invalid params: Unknown sessionId: session_5823".into()), ..Default::default() };
    }
    let send = |p: Progress| {
        let _ = tx.send(Event::Progress(Box::new(p)));
    };
    let mut steps: Vec<Step> = vec![];
    send(Progress { phase: Some("думает".into()), model: Some("demo".into()), session_id: Some(format!("mock-{}", spec.run_id)), ..Default::default() });
    if pause(250, &mut cancel).await {
        return Outcome { stopped: true, ..Default::default() };
    }
    let is_forum = spec.prompt.contains("ФОРУМ");
    let wants_edit = !is_forum && spec.prompt.to_lowercase().contains("файл") || spec.prompt.to_lowercase().contains("правк") || spec.prompt.to_lowercase().contains("edit");
    steps.push(Step { kind: "tool".into(), id: "t1".into(), name: "Read".into(), hint: "Читает README.md".into(), input: json!({"file_path": "README.md"}), status: "running".into(), ..Default::default() });
    send(Progress { phase: Some("работает".into()), steps: Some(steps.clone()), ..Default::default() });
    if pause(400, &mut cancel).await {
        return Outcome { stopped: true, steps, ..Default::default() };
    }
    steps[0].status = "done".into();
    steps[0].output = "# Проект\nДемо-содержимое файла.".into();
    steps[0].ms = Some(400);

    if wants_edit {
        steps.push(Step { kind: "tool".into(), id: "t2".into(), name: "Edit".into(), hint: "Правит demo.txt".into(),
            input: json!({"file_path": "demo.txt", "old_string": "привет", "new_string": "здравствуйте"}), status: "awaiting".into(), ..Default::default() });
        send(Progress { phase: Some("ждёт разрешения".into()), steps: Some(steps.clone()), ..Default::default() });
        let req = PermissionRequest {
            tool_use_id: "t2".into(), tool: "Edit".into(),
            input: json!({"file_path": "demo.txt", "old_string": "привет", "new_string": "здравствуйте"}),
            reason: "Изменить файл demo.txt".into(),
        };
        let pre = approver.preapproved(&req);
        if !pre {
            let _ = tx.send(Event::Ask(req.clone()));
        }
        let d = if pre {
            Decision::Allow
        } else {
            tokio::select! { d = approver.ask(req) => d, _ = cancel.changed() => return Outcome { stopped: true, steps, ..Default::default() } }
        };
        let ok = matches!(d, Decision::Allow);
        if !pre {
            let _ = tx.send(Event::Answered { tool_use_id: "t2".into(), allowed: ok });
        }
        steps[1].status = if ok { "done".into() } else { "denied".into() };
        steps[1].output = if ok { "Файл изменён".into() } else { "Отклонено пользователем".into() };
        steps[1].patch = if ok { json!([{"oldStart": 1, "oldLines": 1, "newStart": 1, "newLines": 1, "lines": ["-привет", "+здравствуйте"]}]) } else { json!(null) };
        send(Progress { phase: Some("думает".into()), steps: Some(steps.clone()), ..Default::default() });
    }

    let slugs: Vec<String> = spec.prompt.split("(@").skip(1).filter_map(|t| t.split(|c| c == ',' || c == ')').next().map(String::from)).collect();
    let answer = if spec.prompt.contains("```senate-plan") {
        "Итог: беремся за работу в два шага.\n\n```senate-plan\n{\"summary\":\"Два шага\",\"decisions\":[{\"title\":\"Идём малыми шагами\",\"body\":\"\",\"scope\":\"project\"}],\"work\":[{\"title\":\"Собрать требования\",\"result\":\"Список требований\",\"criteria\":\"Согласован с человеком\",\"owner\":\"OWNER_A\",\"depends_on\":[]},{\"title\":\"Сделать прототип\",\"result\":\"Рабочий прототип\",\"criteria\":\"Запускается\",\"owner\":\"OWNER_B\",\"depends_on\":[\"Собрать требования\"]}],\"open_questions\":[]}\n```".replace("OWNER_A", slugs.first().map(|x| x.as_str()).unwrap_or("")).replace("OWNER_B", slugs.get(1).map(|x| x.as_str()).unwrap_or(""))
    } else if is_forum {
        "Предлагаю разбить работу на два шага и начать с требований.".to_string()
    } else if wants_edit {
        "Готово. Это демо-ответ: настоящая модель подключается в настройках бэкендов.".to_string()
    } else {
        "Это демо-режим Senate: ответ собран без модели. Подключите Claude Code или Codex в настройках — и специалисты начнут работать по-настоящему.".to_string()
    };
    let mut shown = String::new();
    for word in answer.split_inclusive(' ') {
        shown.push_str(word);
        send(Progress { body: Some(shown.clone()), phase: Some("пишет".into()), ..Default::default() });
        if pause(25, &mut cancel).await {
            return Outcome { stopped: true, text: shown, steps, ..Default::default() };
        }
    }
    Outcome {
        text: answer.into(),
        session_id: Some(format!("mock-{}", spec.run_id)),
        work: Work { input_tokens: 1200, output_tokens: 80, duration_ms: 900, turns: 1, ..Default::default() },
        model: "demo".into(),
        steps,
        ..Default::default()
    }
}

//! Skills of a specialist: ready instructions for work they repeat, with their own scripts and templates.
//!
//! A skill is a folder `<data dir>/skills/<specialist slug>/<skill>/` with a `SKILL.md` (frontmatter `name` and
//! `description`, then the instructions). The specialist is told which skills they have and where; before a task that
//! fits a description they read its `SKILL.md` and follow it. Nothing is loaded into the prompt but the list, so
//! a large skill costs nothing until it is needed.
//!
//! A skill may also carry `routines.json`: work the specialist does on a schedule (cron). They are made once when
//! the skill appears and are the person's from then on: changing the time or switching one off sticks.

use crate::App;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct Skill {
    pub name: String,
    pub description: String,
    pub dir: PathBuf,
    pub routines: Vec<DeclaredRoutine>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct DeclaredRoutine {
    pub name: String,
    pub cron: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub prompt: String,
}

fn safe(s: &str) -> bool {
    !s.is_empty() && s.len() <= 80 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub(crate) fn root(data_dir: &Path, slug: &str) -> PathBuf {
    data_dir.join("skills").join(slug)
}

/// `name` and `description` from the frontmatter between the first two `---` lines.
fn frontmatter(text: &str) -> (String, String) {
    let mut name = String::new();
    let mut description = String::new();
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next().map(str::trim) != Some("---") {
        return (name, description);
    }
    for l in lines {
        if l.trim() == "---" {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            let v = v.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
            match k.trim() {
                "name" => name = v,
                "description" => description = v,
                _ => {}
            }
        }
    }
    (name, description)
}

pub(crate) fn list(data_dir: &Path, slug: &str) -> Vec<Skill> {
    if !safe(slug) {
        return vec![];
    }
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(root(data_dir, slug)) else { return out };
    for e in rd.flatten() {
        let dir = e.path();
        let Ok(text) = std::fs::read_to_string(dir.join("SKILL.md")) else { continue };
        let folder = e.file_name().to_string_lossy().into_owned();
        let (name, description) = frontmatter(&text);
        let routines = std::fs::read_to_string(dir.join("routines.json")).ok().and_then(|t| serde_json::from_str::<Vec<DeclaredRoutine>>(&t).ok()).unwrap_or_default();
        out.push(Skill { name: if name.is_empty() { folder } else { name }, description, dir, routines });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The part of the system prompt that tells the specialist what skills they have.
pub(crate) fn prompt_block(data_dir: &Path, slug: &str) -> String {
    let skills = list(data_dir, slug);
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\n\n## Твои навыки\nНавык — готовая инструкция для работы, которую ты повторяешь, со своими скриптами и шаблонами. \
Когда задача подходит под описание навыка, сначала прочитай его `SKILL.md` инструментом Read и следуй ему: пути внутри навыка считай от его папки, скрипты запускай из неё. \
Файлы навыка не правь без прямой просьбы человека; результаты работы клади в рабочую папку проекта.\n",
    );
    for k in skills {
        let d: String = k.description.chars().take(500).collect();
        s += &format!("- **{}**{}\n  Папка: {}\n", k.name, if d.is_empty() { String::new() } else { format!(" — {d}") }, k.dir.display());
    }
    s
}

pub(crate) fn block_for(data_dir: &Path, slug: &str) -> String {
    prompt_block(data_dir, slug)
}

/// The machine's own UTC offset in seconds: cron is read in local time.
pub(crate) fn offset_secs() -> i64 {
    chrono::Local::now().offset().local_minus_utc() as i64
}

/// Make the routines skills declare. Cheap and idempotent: run at start and now and then.
pub(crate) fn sync_routines(app: &App) {
    let Ok(agents) = app.store.list_agents(false) else { return };
    let offset = offset_secs();
    let mut made = 0;
    for a in agents {
        for k in list(&app.cfg.data_dir, &a.slug) {
            for r in &k.routines {
                let title = if r.title.is_empty() { r.name.clone() } else { r.title.clone() };
                match app.store.ensure_routine(&a.id, &r.name, &r.cron, &title, &r.prompt, &format!("skill:{}", k.name), offset) {
                    Ok((_, true)) => made += 1,
                    Ok(_) => {}
                    Err(e) => tracing::warn!("навык {}: регулярная задача «{}»: {e}", k.name, r.name),
                }
            }
        }
    }
    if made > 0 {
        app.publish("routines", serde_json::json!({}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("senate-skills-{}", senate_core::new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn skill(data: &Path, slug: &str, folder: &str, body: &str) -> PathBuf {
        let dir = data.join("skills").join(slug).join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
        dir
    }

    #[test]
    fn a_skill_is_found_by_its_folder_and_described_by_its_frontmatter() {
        let d = tmp();
        skill(&d, "seo", "seo-wizard", "---\nname: seo-wizard\ndescription: \"Недельный SEO-сценарий: понедельник, четверг\"\n---\n# SEO");
        skill(&d, "seo", "no-front", "# просто текст");
        let list = list(&d, "seo");
        assert_eq!(list.len(), 2);
        let w = list.iter().find(|s| s.name == "seo-wizard").unwrap();
        assert_eq!(w.description, "Недельный SEO-сценарий: понедельник, четверг");
        assert!(list.iter().any(|s| s.name == "no-front"), "no frontmatter: the folder name stands in");
        // the prompt lists them and says where, without loading their text
        let p = prompt_block(&d, "seo");
        assert!(p.contains("## Твои навыки") && p.contains("seo-wizard") && p.contains("Папка:"));
        assert!(!p.contains("# SEO"), "the body of a skill is not in the prompt");
        // somebody else has none; odd slugs find nothing
        assert!(prompt_block(&d, "other").is_empty());
        assert!(list_for_bad(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    fn list_for_bad(d: &Path) -> Vec<Skill> {
        list(d, "../seo")
    }

    #[test]
    fn declared_routines_are_read_and_bad_files_are_ignored() {
        let d = tmp();
        let dir = skill(&d, "seo", "seo-wizard", "---\nname: seo-wizard\n---\n");
        std::fs::write(dir.join("routines.json"), r#"[{"name":"Понедельник","cron":"0 9 * * 1","title":"SEO: понедельник","prompt":"Разбери"},{"name":"Четверг","cron":"0 9 * * 4"}]"#).unwrap();
        let s = &list(&d, "seo")[0];
        assert_eq!(s.routines.len(), 2);
        assert_eq!(s.routines[1].title, "", "title is optional");
        std::fs::write(dir.join("routines.json"), "not json").unwrap();
        assert!(list(&d, "seo")[0].routines.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}

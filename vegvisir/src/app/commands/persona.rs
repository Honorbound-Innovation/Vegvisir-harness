use crate::{
    app::{
        PendingEditorAction, PendingEditorKind, apply_persona_to_system_prompt,
        strip_persona_from_system_prompt,
    },
    persona::{
        DEFAULT_PERSONA_ID, draft_persona, get_builtin_persona, get_persona_with_root,
        import_persona_file, list_personas_with_root, persona_path, render_persona_prompt_section,
        save_custom_persona,
    },
};

use super::super::TuiApplication;

impl TuiApplication {
    pub(crate) fn persona_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        match args.first().map(String::as_str) {
            None | Some("show") | Some("current") => self.ka_show(args.get(1).map(String::as_str)),
            Some("list") | Some("ls") => self.ka_list(),
            Some("set") | Some("use") | Some("select") => {
                let Some(id) = args.get(1) else {
                    return Ok("Usage: /ka set <id>".to_string());
                };
                self.ka_set(id)
            }
            Some("clear") | Some("off") | Some("none") => self.ka_clear(),
            Some("default") | Some("reset") => self.ka_default(),
            Some("create") | Some("new") => self.ka_create(&args[1..]),
            Some("import") => self.ka_import(&args[1..]),
            Some("edit") => self.ka_edit(&args[1..]),
            Some(other) => {
                if get_persona_with_root(&self.data_root, other)?.is_some() {
                    self.ka_set(other)
                } else {
                    Ok(format!(
                        "Unknown ka command: {other}\nUsage: /ka [list|show [id]|set <id>|create <id> [display name]|import <path>|edit <id>|clear|default]\nCompatibility alias: /persona"
                    ))
                }
            }
        }
    }

    fn ka_show(&self, requested: Option<&str>) -> anyhow::Result<String> {
        let id = requested
            .or(self.session.active_persona_id.as_deref())
            .unwrap_or(DEFAULT_PERSONA_ID);
        let Some(profile) = get_persona_with_root(&self.data_root, id)? else {
            return Ok(format!(
                "Unknown ka: {id}\nAvailable ka profiles:\n{}",
                self.ka_list()?
            ));
        };
        Ok(format!(
            "active_ka: {}\n\n{}",
            self.session
                .active_persona_id
                .as_deref()
                .unwrap_or(DEFAULT_PERSONA_ID),
            render_persona_prompt_section(&profile)
        ))
    }

    fn ka_list(&self) -> anyhow::Result<String> {
        Ok(list_personas_with_root(&self.data_root)?
            .iter()
            .map(|profile| {
                let kind = if get_builtin_persona(&profile.id).is_some() {
                    "built-in"
                } else {
                    "custom"
                };
                format!(
                    "{:<20} {:<10} {:<24} {}",
                    profile.id, kind, profile.display_name, profile.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn ka_set(&mut self, id: &str) -> anyhow::Result<String> {
        let Some(profile) = get_persona_with_root(&self.data_root, id)? else {
            return Ok(format!(
                "Unknown ka: {id}\nAvailable ka profiles:\n{}",
                self.ka_list()?
            ));
        };
        let base = strip_persona_from_system_prompt(&self.session.system_prompt);
        self.session.active_persona_id = Some(profile.id.clone());
        self.session.system_prompt =
            apply_persona_to_system_prompt(&base, Some(&profile.id), &self.data_root);
        self.save_config_defaults()?;
        self.autosave_session();
        Ok(format!(
            "Ka set to {} ({})\nKa/persona affects delivery style only; USRL, tool policy, approval policy, secrets policy, verification requirements, and user authority remain higher priority.",
            profile.id, profile.display_name
        ))
    }

    fn ka_clear(&mut self) -> anyhow::Result<String> {
        self.session.system_prompt = strip_persona_from_system_prompt(&self.session.system_prompt);
        self.session.active_persona_id = None;
        self.save_config_defaults()?;
        self.autosave_session();
        Ok("Ka cleared. The harness system prompt remains active without an appended ka/persona section.".to_string())
    }

    fn ka_default(&mut self) -> anyhow::Result<String> {
        let base = strip_persona_from_system_prompt(&self.session.system_prompt);
        self.session.active_persona_id = Some(DEFAULT_PERSONA_ID.to_string());
        self.session.system_prompt =
            apply_persona_to_system_prompt(&base, Some(DEFAULT_PERSONA_ID), &self.data_root);
        self.save_config_defaults()?;
        self.autosave_session();
        Ok("Ka reset to vegvisir_default.".to_string())
    }

    fn ka_create(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(id) = args.first() else {
            return Ok("Usage: /ka create <id> [display name...]".to_string());
        };
        let display = if args.len() > 1 {
            args[1..].join(" ")
        } else {
            title_from_id(id)
        };
        let profile = draft_persona(id, &display);
        let path = save_custom_persona(&self.data_root, &profile)?;
        Ok(format!(
            "Created custom ka `{}` at {}\nEdit it with: /ka edit {}\nActivate it with: /ka set {}",
            profile.id,
            path.display(),
            profile.id,
            profile.id
        ))
    }

    fn ka_import(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(path) = args.first() else {
            return Ok("Usage: /ka import <json-or-yaml-path>".to_string());
        };
        let path = self.resolve_workspace_path(path);
        let saved = import_persona_file(&self.data_root, &path)?;
        Ok(format!("Imported ka profile to {}", saved.display()))
    }

    fn ka_edit(&mut self, args: &[String]) -> anyhow::Result<String> {
        let Some(id) = args.first() else {
            return Ok("Usage: /ka edit <id>".to_string());
        };
        if get_builtin_persona(id).is_some() {
            return Ok(format!(
                "`{}` is built-in and cannot be edited directly. Create a custom copy: /ka create my_{}",
                id, id
            ));
        }
        let path = persona_path(&self.data_root, id);
        if !path.exists() {
            let profile = draft_persona(id, &title_from_id(id));
            save_custom_persona(&self.data_root, &profile)?;
        }
        self.pending_editor_action = Some(PendingEditorAction {
            kind: PendingEditorKind::KaProfile,
            id: id.to_string(),
            path: path.clone(),
        });
        Ok(format!(
            "Opening editor for ka `{}` at {}. Vegvisir will temporarily restore the terminal and then resume the TUI when the editor exits.",
            id,
            path.display()
        ))
    }
}

fn title_from_id(id: &str) -> String {
    id.replace(['-', '_'], " ")
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

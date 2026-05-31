use crate::profile::UserProfile;

use super::super::TuiApplication;

impl TuiApplication {
    pub(crate) fn profile_command(&mut self, args: &[String]) -> anyhow::Result<String> {
        let action = args.first().map(String::as_str).unwrap_or("show");
        match action {
            "show" | "status" => Ok(self.user_profile.summary(self.profile_store.path())),
            "path" => Ok(format!(
                "User profile path: {}",
                self.profile_store.path().display()
            )),
            "init" => {
                self.profile_store.save(&self.user_profile)?;
                Ok(format!(
                    "Initialized user profile at {}",
                    self.profile_store.path().display()
                ))
            }
            "help" => Ok(profile_help()),
            "set" => {
                let field = args.get(1).map(String::as_str).unwrap_or("");
                let value = args.get(2).map(String::as_str).unwrap_or("");
                if field.is_empty() || value.is_empty() {
                    anyhow::bail!("Usage: /profile set <field> <value>");
                }
                self.user_profile.set_field(field, value)?;
                self.profile_store.save(&self.user_profile)?;
                Ok(format!(
                    "Updated profile field `{field}`.\n\n{}",
                    self.user_profile.summary(self.profile_store.path())
                ))
            }
            "add" => {
                let field = args.get(1).map(String::as_str).unwrap_or("");
                let value = args.get(2).map(String::as_str).unwrap_or("");
                if field.is_empty() || value.is_empty() {
                    anyhow::bail!(
                        "Usage: /profile add <spoken_languages|coding_languages> <value>"
                    );
                }
                self.user_profile.add_list_value(field, value)?;
                self.profile_store.save(&self.user_profile)?;
                Ok(format!(
                    "Added `{value}` to profile `{field}`.\n\n{}",
                    self.user_profile.summary(self.profile_store.path())
                ))
            }
            "remove" | "rm" => {
                let field = args.get(1).map(String::as_str).unwrap_or("");
                let value = args.get(2).map(String::as_str).unwrap_or("");
                if field.is_empty() || value.is_empty() {
                    anyhow::bail!(
                        "Usage: /profile remove <spoken_languages|coding_languages> <value>"
                    );
                }
                self.user_profile.remove_list_value(field, value)?;
                self.profile_store.save(&self.user_profile)?;
                Ok(format!(
                    "Removed `{value}` from profile `{field}`.\n\n{}",
                    self.user_profile.summary(self.profile_store.path())
                ))
            }
            "clear" | "reset" => {
                self.user_profile = UserProfile::default();
                self.profile_store.save(&self.user_profile)?;
                Ok(format!(
                    "Reset user profile at {}",
                    self.profile_store.path().display()
                ))
            }
            other => anyhow::bail!("Unknown profile action '{other}'. Use /profile help."),
        }
    }
}

fn profile_help() -> String {
    r#"User profile commands

/profile show
/profile path
/profile init
/profile set name <display-name>
/profile set address_as <preferred-name>
/profile set pronouns <pronouns>
/profile set spoken_language <language-code-or-name>
/profile set response_language <language-code-or-name>
/profile set tone <style>
/profile set verbosity <short|medium|detailed|...>
/profile set use_name_in_chat <true|false>
/profile add spoken_languages <language>
/profile add coding_languages <language>
/profile remove spoken_languages <language>
/profile remove coding_languages <language>
/profile set comment_language <language>
/profile set code_style <style>
/profile set test_preference <preference>
/profile set show_code_and_diffs_in_chat <true|false>
/profile set commit_and_push_when_clean <true|false>
/profile set prefer_direct_implementation <true|false>
/profile set autonomy.default_max_steps <n>
/profile set autonomy.default_max_attempts <n>
/profile clear

The profile is local, user-controlled, non-secret data stored as JSON.
CMS sync is intentionally not part of this implementation yet."#
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_command_sets_and_persists_preferences() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&workspace)?;
        let data_root = tmp.path().join("home");
        let mut app = TuiApplication::with_data_root(&workspace, &data_root)?;

        let response =
            app.profile_command(&["set".to_string(), "name".to_string(), "Malice".to_string()])?;
        assert!(response.contains("name: Malice"));
        app.profile_command(&[
            "add".to_string(),
            "coding_languages".to_string(),
            "Rust".to_string(),
        ])?;

        let reloaded = app.profile_store.load()?;
        assert_eq!(reloaded.identity.display_name, "Malice");
        assert_eq!(reloaded.coding.coding_languages, vec!["Rust".to_string()]);
        Ok(())
    }
}

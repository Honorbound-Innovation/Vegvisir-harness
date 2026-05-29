use crate::models::*;
pub fn builtin_profiles() -> Vec<DomainProfile> {
    vec![
        profile(
            "generic-technical-docs",
            vec!["Technical Documentation Agent"],
            vec![],
            vec!["procedure", "troubleshooting", "reference"],
            vec!["source", "citation", "version"],
        ),
        profile(
            "api-operations",
            vec!["API Operations Agent"],
            vec!["curl", "httpie", "jq"],
            vec!["endpoint review", "request validation", "error handling"],
            vec!["endpoint", "method", "schema", "auth"],
        ),
        profile(
            "cli-operations",
            vec!["CLI Operations Agent"],
            vec!["bash", "jq", "grep"],
            vec!["status check", "dry run", "diagnostic command"],
            vec!["flag", "subcommand", "exit code", "dry-run"],
        ),
        profile(
            "kubernetes-operations",
            vec!["Cluster Diagnostic Agent", "Manifest Review Agent"],
            vec!["kubectl", "helm", "stern", "jq"],
            vec![
                "diagnose",
                "review manifests",
                "inspect events",
                "rollout safety",
            ],
            vec![
                "pod",
                "deployment",
                "service",
                "namespace",
                "manifest",
                "rollout",
            ],
        ),
        profile(
            "bethesda-modding",
            vec![
                "Load Order Analyst",
                "Conflict Resolution Agent",
                "Papyrus Scripting Agent",
            ],
            vec![
                "xEdit",
                "LOOT",
                "Mod Organizer 2",
                "Creation Kit",
                "Papyrus Compiler",
            ],
            vec![
                "load order review",
                "conflict analysis",
                "script diagnosis",
                "packaging",
            ],
            vec![
                "plugin",
                "load order",
                "record",
                "override",
                "Papyrus",
                "archive",
            ],
        ),
        profile(
            "unreal-engine",
            vec![
                "Unreal C++ Engineer",
                "Blueprint Systems Designer",
                "Unreal Build Troubleshooter",
            ],
            vec![
                "Unreal Editor",
                "UnrealBuildTool",
                "AutomationTool",
                "Unreal Insights",
            ],
            vec![
                "build troubleshooting",
                "Blueprint workflow",
                "packaging",
                "profiling",
            ],
            vec![
                "Blueprint",
                "Actor",
                "module",
                "packaging",
                "replication",
                "profiling",
            ],
        ),
    ]
}
fn profile(
    name: &str,
    roles: Vec<&str>,
    tools: Vec<&str>,
    task_types: Vec<&str>,
    terminology: Vec<&str>,
) -> DomainProfile {
    DomainProfile {
        name: name.into(),
        preferred_skill_types: vec![
            SkillType::Procedure,
            SkillType::Diagnostic,
            SkillType::ToolUse,
        ],
        known_tools: tools.into_iter().map(str::to_string).collect(),
        risk_categories: vec![
            "mutation".into(),
            "version-specific".into(),
            "tool-permission".into(),
        ],
        common_task_types: task_types.into_iter().map(str::to_string).collect(),
        common_anti_patterns: vec![
            "Treating version-specific docs as universal".into(),
            "Running mutating tools without backup, dry-run, or approval context".into(),
        ],
        preferred_agent_roles: roles.into_iter().map(str::to_string).collect(),
        source_trust_hierarchy: vec![
            SourceTrust::OfficialVendorDocumentation,
            SourceTrust::OfficialApiSpecification,
            SourceTrust::OfficialCliReference,
            SourceTrust::ProjectMaintainerDocumentation,
            SourceTrust::RepositoryTestsAndExamples,
            SourceTrust::CommunityGuide,
        ],
        terminology: terminology.into_iter().map(str::to_string).collect(),
        required_review_policy: "Operational, mutating, inferred, or low-confidence skills require verifier/human review before agent-builder default use.".into(),
    }
}
pub fn get_profile(name: &str) -> Option<DomainProfile> {
    builtin_profiles().into_iter().find(|p| p.name == name)
}

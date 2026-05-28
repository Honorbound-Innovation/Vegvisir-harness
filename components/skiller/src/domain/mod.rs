use crate::models::*;
pub fn builtin_profiles() -> Vec<DomainProfile> {
    vec![
        profile(
            "generic-technical-docs",
            vec!["Technical Documentation Agent"],
        ),
        profile("api-operations", vec!["API Operations Agent"]),
        profile("cli-operations", vec!["CLI Operations Agent"]),
        profile(
            "kubernetes-operations",
            vec!["Cluster Diagnostic Agent", "Manifest Review Agent"],
        ),
        profile(
            "bethesda-modding",
            vec![
                "Load Order Analyst",
                "Conflict Resolution Agent",
                "Papyrus Scripting Agent",
            ],
        ),
        profile(
            "unreal-engine",
            vec![
                "Unreal C++ Engineer",
                "Blueprint Systems Designer",
                "Unreal Build Troubleshooter",
            ],
        ),
    ]
}
fn profile(name: &str, roles: Vec<&str>) -> DomainProfile {
    DomainProfile {
        name: name.into(),
        preferred_skill_types: vec![
            SkillType::Procedure,
            SkillType::Diagnostic,
            SkillType::ToolUse,
        ],
        known_tools: vec![],
        risk_categories: vec!["mutation".into(), "version-specific".into()],
        common_task_types: vec!["diagnose".into(), "review".into(), "operate".into()],
        common_anti_patterns: vec!["Treating version-specific docs as universal".into()],
        preferred_agent_roles: roles.into_iter().map(str::to_string).collect(),
        source_trust_hierarchy: vec![
            SourceTrust::OfficialVendorDocumentation,
            SourceTrust::ProjectMaintainerDocumentation,
            SourceTrust::CommunityGuide,
        ],
        terminology: vec![],
        required_review_policy: "Operational or mutating skills require human review.".into(),
    }
}
pub fn get_profile(name: &str) -> Option<DomainProfile> {
    builtin_profiles().into_iter().find(|p| p.name == name)
}

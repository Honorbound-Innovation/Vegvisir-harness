use crate::models::{SourceDocument, SourceTrust, SourceType, Visibility};

pub fn infer_source_trust(source: &SourceDocument) -> SourceTrust {
    match source.source_type {
        SourceType::OpenApi | SourceType::ApiSpec => SourceTrust::OfficialApiSpecification,
        SourceType::CliHelp | SourceType::CliSpec => SourceTrust::OfficialCliReference,
        SourceType::Repository => SourceTrust::ProjectMaintainerDocumentation,
        SourceType::Unknown => SourceTrust::UnknownSource,
        SourceType::Url | SourceType::Html => infer_web_source_trust(source),
        SourceType::Markdown | SourceType::Text => infer_document_source_trust(source),
        SourceType::Pdf | SourceType::Epub => infer_document_source_trust(source),
    }
}

fn infer_web_source_trust(source: &SourceDocument) -> SourceTrust {
    let origin = source.origin.to_lowercase();
    if origin.contains("community") || origin.contains("forum") || origin.contains("reddit") {
        SourceTrust::CommunityGuide
    } else if origin.contains("official")
        || origin.contains("docs.")
        || origin.contains("developer.")
    {
        SourceTrust::OfficialVendorDocumentation
    } else if matches!(
        source.visibility,
        Visibility::Internal | Visibility::Restricted
    ) {
        SourceTrust::InternalCompanyDocumentation
    } else {
        SourceTrust::UnknownSource
    }
}

fn infer_document_source_trust(source: &SourceDocument) -> SourceTrust {
    let combined = format!("{} {}", source.title, source.origin).to_lowercase();
    if combined.contains("community") || combined.contains("guide") {
        SourceTrust::CommunityGuide
    } else if combined.contains("official") || combined.contains("vendor manual") {
        SourceTrust::OfficialVendorDocumentation
    } else if matches!(
        source.visibility,
        Visibility::Internal | Visibility::Restricted
    ) {
        SourceTrust::InternalCompanyDocumentation
    } else {
        SourceTrust::ProjectMaintainerDocumentation
    }
}

pub fn source_trust_score(source: Option<&SourceDocument>) -> f32 {
    match source.map(infer_source_trust) {
        Some(SourceTrust::OfficialVendorDocumentation)
        | Some(SourceTrust::OfficialApiSpecification)
        | Some(SourceTrust::OfficialCliReference)
        | Some(SourceTrust::OfficialStandardsDocument) => 0.85,
        Some(SourceTrust::ProjectMaintainerDocumentation)
        | Some(SourceTrust::RepositoryTestsAndExamples)
        | Some(SourceTrust::InternalCompanyDocumentation) => 0.72,
        Some(SourceTrust::CommunityGuide) | Some(SourceTrust::GeneratedDocumentation) => 0.55,
        Some(SourceTrust::ForumPost) | Some(SourceTrust::UnknownSource) => 0.4,
        Some(SourceTrust::ArchivedOrStaleSource) => 0.25,
        None => 0.35,
    }
}

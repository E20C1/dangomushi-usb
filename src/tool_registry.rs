#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolId {
    Files,
    Editor,
    HashVerify,
    TextJson,
    Compare,
    Search,
    Repositories,
    Backup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Ready,
    Planned,
}

impl ToolStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::Planned => "PLANNED",
        }
    }

    pub fn is_ready(self) -> bool {
        self == Self::Ready
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCategory {
    Workspace,
    Utility,
    VersionControl,
    Recovery,
}

impl ToolCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Workspace => "WORKSPACE",
            Self::Utility => "UTILITY",
            Self::VersionControl => "VERSION CONTROL",
            Self::Recovery => "RECOVERY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDefinition {
    pub id: ToolId,
    pub key: &'static str,
    pub label: &'static str,
    pub category: ToolCategory,
    pub description: &'static str,
    pub status: ToolStatus,
}

const TOOLS: &[ToolDefinition] = &[
    ToolDefinition {
        id: ToolId::Files,
        key: "files",
        label: "FILES",
        category: ToolCategory::Workspace,
        description: "Browse the configured workspace without escaping its filesystem boundary.",
        status: ToolStatus::Ready,
    },
    ToolDefinition {
        id: ToolId::Editor,
        key: "editor",
        label: "EDITOR",
        category: ToolCategory::Workspace,
        description: "Edit existing UTF-8 text files with explicit save and recovery autosave.",
        status: ToolStatus::Ready,
    },
    ToolDefinition {
        id: ToolId::HashVerify,
        key: "hash-verify",
        label: "HASH / VERIFY",
        category: ToolCategory::Utility,
        description: "Calculate and verify SHA-256 digests for local Workspace files.",
        status: ToolStatus::Ready,
    },
    ToolDefinition {
        id: ToolId::TextJson,
        key: "text-json",
        label: "TEXT / JSON",
        category: ToolCategory::Utility,
        description: "Normalize text and validate or format JSON entirely in memory.",
        status: ToolStatus::Ready,
    },
    ToolDefinition {
        id: ToolId::Compare,
        key: "compare",
        label: "COMPARE",
        category: ToolCategory::Utility,
        description: "Compare two Workspace files locally with binary-safe streaming.",
        status: ToolStatus::Ready,
    },
    ToolDefinition {
        id: ToolId::Search,
        key: "search",
        label: "SEARCH",
        category: ToolCategory::Utility,
        description: "Search UTF-8 documents under data/documents entirely on this device.",
        status: ToolStatus::Ready,
    },
    ToolDefinition {
        id: ToolId::Repositories,
        key: "repositories",
        label: "REPOSITORIES",
        category: ToolCategory::VersionControl,
        description: "Inspect repository state and expose bounded Git operations.",
        status: ToolStatus::Planned,
    },
    ToolDefinition {
        id: ToolId::Backup,
        key: "backup",
        label: "BACKUP",
        category: ToolCategory::Recovery,
        description: "Create and verify local workspace recovery snapshots.",
        status: ToolStatus::Planned,
    },
];

pub fn all() -> &'static [ToolDefinition] {
    TOOLS
}

pub fn by_key(key: &str) -> Option<&'static ToolDefinition> {
    TOOLS.iter().find(|tool| tool.key == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn registry_keys_are_non_empty_and_unique() {
        let mut keys = BTreeSet::new();

        for tool in all() {
            assert!(!tool.key.trim().is_empty(), "tool key must not be empty");
            assert!(
                keys.insert(tool.key),
                "duplicate tool registry key: {}",
                tool.key
            );
        }
    }

    #[test]
    fn implemented_tools_are_ready() {
        assert_eq!(
            by_key("files").map(|tool| tool.status),
            Some(ToolStatus::Ready)
        );
        assert_eq!(
            by_key("editor").map(|tool| tool.status),
            Some(ToolStatus::Ready)
        );
        assert_eq!(
            by_key("hash-verify").map(|tool| tool.status),
            Some(ToolStatus::Ready)
        );
        assert_eq!(
            by_key("text-json").map(|tool| tool.status),
            Some(ToolStatus::Ready)
        );
        assert_eq!(
            by_key("compare").map(|tool| tool.status),
            Some(ToolStatus::Ready)
        );
        assert_eq!(
            by_key("search").map(|tool| tool.status),
            Some(ToolStatus::Ready)
        );
    }

    #[test]
    fn stable_key_lookup_returns_expected_tool() {
        assert_eq!(
            by_key("hash-verify").map(|tool| tool.id),
            Some(ToolId::HashVerify)
        );
        assert_eq!(
            by_key("text-json").map(|tool| tool.id),
            Some(ToolId::TextJson)
        );
        assert_eq!(
            by_key("compare").map(|tool| tool.id),
            Some(ToolId::Compare)
        );
        assert_eq!(
            by_key("search").map(|tool| tool.id),
            Some(ToolId::Search)
        );
        assert!(by_key("missing-tool").is_none());
    }
}

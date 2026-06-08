#[cfg(not(target_arch = "wasm32"))]
use std::ffi::{OsStr, OsString};
#[cfg(not(target_arch = "wasm32"))]
use std::fs::{self, OpenOptions};
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
#[cfg(all(not(target_arch = "wasm32"), unix))]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use witty_core::GridSize;

#[cfg(not(target_arch = "wasm32"))]
use crate::{LocalPtyConfig, LocalPtyTransport, TerminalTransport, TransportEvent};

pub const PROFILE_STORE_SCHEMA_V1: u16 = 1;
pub const PROFILE_STORE_APP: &str = "witty-profiles";
pub const PROFILE_STORE_MAX_JSON_BYTES: usize = 1024 * 1024;
pub const PROFILE_STORE_MAX_PROFILES: usize = 512;
pub const PROFILE_STORE_MAX_TAGS_PER_PROFILE: usize = 32;
pub const PROFILE_STORE_MAX_OPENSSH_EXTRA_ARGS: usize = 64;
pub const PROFILE_STORE_MAX_REMOTE_COMMAND_ARGS: usize = 64;
#[cfg(not(target_arch = "wasm32"))]
const PROFILE_STORE_TEMP_FILE_RETRIES: usize = 16;
#[cfg(not(target_arch = "wasm32"))]
static PROFILE_STORE_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenSshProfile {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    pub config_file: Option<PathBuf>,
    pub jump_host: Option<String>,
    pub term: Option<String>,
    pub request_tty: bool,
    pub extra_args: Vec<String>,
    pub remote_command: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SshProfile {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub target: SshProfileTarget,
    pub credential: SshCredentialRef,
    pub terminal: SshTerminalOptions,
    pub openssh: OpenSshAdvancedOptions,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SshProfileTarget {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub jump_host: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SshCredentialRef {
    #[default]
    DefaultAgent,
    IdentityFile {
        path: PathBuf,
    },
    VaultSecret {
        secret_id: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SshTerminalOptions {
    pub term: Option<String>,
    pub request_tty: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenSshAdvancedOptions {
    pub config_file: Option<PathBuf>,
    pub extra_args: Vec<String>,
    pub remote_command: Vec<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenSshImportPreview {
    pub candidates: Vec<OpenSshImportCandidate>,
    pub warnings: Vec<OpenSshImportWarning>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenSshImportCandidate {
    pub profile: SshProfile,
    pub source: OpenSshImportSource,
    pub warnings: Vec<OpenSshImportWarning>,
    pub conflict: Option<OpenSshImportConflict>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct OpenSshImportSource {
    pub config_path: Option<PathBuf>,
    pub host_pattern: String,
    pub line: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenSshImportConflict {
    ExistingProfileId { profile_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenSshImportWarning {
    UnsupportedDirective { keyword: String },
    UnsupportedPattern { pattern: String },
    InvalidDirectiveValue { keyword: String, value: String },
    MissingHostName { host_pattern: String },
    SkippedWildcardOnlyHost { pattern: String },
    EmptyProfileId { host_pattern: String },
    TokenExpansionPreserved { field: String },
    IdentityFilePathOnly,
    RemoteCommandImported,
    ProxyCommandNotImported,
    IncludeNotExpanded { path: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenSshImportConflictPolicy {
    Reject,
    Replace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenSshImportSelection {
    All,
    ProfileIds(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenSshImportApplyReport {
    pub mutation: ProfileStoreMutation,
    pub selected: usize,
    pub added: usize,
    pub replaced: usize,
    pub candidate_warning_count: usize,
    pub global_warning_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenSshImportReview {
    pub candidates: Vec<OpenSshImportCandidateSummary>,
    pub selected_by_default: Vec<String>,
    pub warning_count: usize,
    pub global_warning_count: usize,
    pub conflict_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenSshImportCandidateSummary {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub warning_count: usize,
    pub has_conflict: bool,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenSshConfigDumpSmoke {
    pub destination: String,
    pub exit_code: Option<i32>,
    pub output: String,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileStoreWriteReport {
    pub path: PathBuf,
    pub bytes_written: usize,
    pub created_parent_dir: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileStoreDefaultPolicy {
    Preserve,
    SetIfEmpty,
    SetToAdded,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfileStoreMutation {
    pub changed: bool,
    pub profile_count: usize,
    pub default_profile_changed: bool,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileStoreEditOpenMode {
    Existing,
    CreateIfMissing,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileStoreEditReport {
    pub write: ProfileStoreWriteReport,
    pub mutation: ProfileStoreMutation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileStoreV1 {
    pub schema: u16,
    pub app: String,
    pub profiles: Vec<SshProfile>,
    pub default_profile_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileStoreSummary {
    pub profiles: Vec<ProfileSummary>,
    pub default_profile_id: Option<String>,
    pub launchable_profiles: usize,
    pub credential_resolver_required_profiles: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub launchability: SshProfileLaunchability,
    pub is_default: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfileStoreValidation {
    pub launchable_profiles: usize,
    pub credential_resolver_required_profiles: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SshProfileLaunchability {
    Launchable,
    RequiresCredentialResolver,
}

impl Default for ProfileStoreV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfileStoreV1 {
    pub fn new() -> Self {
        Self {
            schema: PROFILE_STORE_SCHEMA_V1,
            app: PROFILE_STORE_APP.to_owned(),
            profiles: Vec::new(),
            default_profile_id: None,
        }
    }

    pub fn with_profiles(profiles: Vec<SshProfile>) -> Self {
        Self {
            profiles,
            ..Self::new()
        }
    }

    pub fn from_json(json: &str) -> Result<Self> {
        if json.len() > PROFILE_STORE_MAX_JSON_BYTES {
            bail!(
                "profile store JSON exceeds {} bytes",
                PROFILE_STORE_MAX_JSON_BYTES
            );
        }

        let store: Self =
            serde_json::from_str(json).context("parse profile store JSON as schema v1")?;
        store.validate()?;
        Ok(store)
    }

    pub fn to_pretty_json(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string_pretty(self).context("serialize profile store JSON")
    }

    pub fn validate(&self) -> Result<ProfileStoreValidation> {
        if self.schema != PROFILE_STORE_SCHEMA_V1 {
            bail!(
                "unsupported profile store schema {}; expected {}",
                self.schema,
                PROFILE_STORE_SCHEMA_V1
            );
        }
        if self.app != PROFILE_STORE_APP {
            bail!(
                "invalid profile store app {}; expected {}",
                self.app,
                PROFILE_STORE_APP
            );
        }
        if self.profiles.len() > PROFILE_STORE_MAX_PROFILES {
            bail!(
                "profile store has {} profiles; maximum is {}",
                self.profiles.len(),
                PROFILE_STORE_MAX_PROFILES
            );
        }

        let mut ids = BTreeSet::new();
        let mut default_profile_seen = self.default_profile_id.is_none();
        if let Some(default_profile_id) = &self.default_profile_id {
            validate_ssh_atom("default profile id", default_profile_id)?;
        }

        let mut validation = ProfileStoreValidation::default();
        for profile in &self.profiles {
            validate_profile_store_limits(profile)?;
            match profile.launchability()? {
                SshProfileLaunchability::Launchable => validation.launchable_profiles += 1,
                SshProfileLaunchability::RequiresCredentialResolver => {
                    validation.credential_resolver_required_profiles += 1;
                }
            }

            if !ids.insert(profile.id.clone()) {
                bail!("duplicate profile id {}", profile.id);
            }
            if self.default_profile_id.as_deref() == Some(profile.id.as_str()) {
                default_profile_seen = true;
            }
        }

        if !default_profile_seen {
            bail!(
                "default profile id {} does not exist",
                self.default_profile_id
                    .as_deref()
                    .expect("checked missing default profile above")
            );
        }

        Ok(validation)
    }

    pub fn redacted_summary(&self) -> Result<ProfileStoreSummary> {
        let validation = self.validate()?;
        Ok(ProfileStoreSummary {
            profiles: self
                .profiles
                .iter()
                .map(|profile| {
                    Ok(ProfileSummary {
                        id: profile.id.clone(),
                        name: profile.name.clone(),
                        tags: profile.tags.clone(),
                        launchability: profile.launchability()?,
                        is_default: self.default_profile_id.as_deref() == Some(profile.id.as_str()),
                    })
                })
                .collect::<Result<Vec<_>>>()?,
            default_profile_id: self.default_profile_id.clone(),
            launchable_profiles: validation.launchable_profiles,
            credential_resolver_required_profiles: validation.credential_resolver_required_profiles,
        })
    }

    pub fn profile(&self, id: &str) -> Option<&SshProfile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    pub fn add_profile(
        &mut self,
        profile: SshProfile,
        default_policy: ProfileStoreDefaultPolicy,
    ) -> Result<ProfileStoreMutation> {
        if self.profile(&profile.id).is_some() {
            bail!("profile id {} already exists", profile.id);
        }

        let before = self.clone();
        let mut next = self.clone();
        let profile_id = profile.id.clone();
        next.profiles.push(profile);
        match default_policy {
            ProfileStoreDefaultPolicy::Preserve => {}
            ProfileStoreDefaultPolicy::SetIfEmpty => {
                if next.default_profile_id.is_none() {
                    next.default_profile_id = Some(profile_id);
                }
            }
            ProfileStoreDefaultPolicy::SetToAdded => {
                next.default_profile_id = Some(profile_id);
            }
        }

        next.validate()?;
        let mutation = ProfileStoreMutation::from_stores(&before, &next);
        *self = next;
        Ok(mutation)
    }

    pub fn update_profile(
        &mut self,
        id: &str,
        profile: SshProfile,
    ) -> Result<ProfileStoreMutation> {
        validate_ssh_atom("profile id", id)?;
        if profile.id != id {
            bail!("profile id changes require a rename operation");
        }

        let before = self.clone();
        let mut next = self.clone();
        let Some(existing) = next
            .profiles
            .iter_mut()
            .find(|candidate| candidate.id == id)
        else {
            bail!("profile id {id} does not exist");
        };
        *existing = profile;

        next.validate()?;
        let mutation = ProfileStoreMutation::from_stores(&before, &next);
        *self = next;
        Ok(mutation)
    }

    pub fn remove_profile(&mut self, id: &str) -> Result<ProfileStoreMutation> {
        validate_ssh_atom("profile id", id)?;

        let before = self.clone();
        let mut next = self.clone();
        let Some(index) = next
            .profiles
            .iter()
            .position(|candidate| candidate.id == id)
        else {
            bail!("profile id {id} does not exist");
        };
        next.profiles.remove(index);
        if next.default_profile_id.as_deref() == Some(id) {
            next.default_profile_id = None;
        }

        next.validate()?;
        let mutation = ProfileStoreMutation::from_stores(&before, &next);
        *self = next;
        Ok(mutation)
    }

    pub fn set_default_profile(&mut self, id: Option<&str>) -> Result<ProfileStoreMutation> {
        if let Some(id) = id {
            validate_ssh_atom("profile id", id)?;
            if self.profile(id).is_none() {
                bail!("profile id {id} does not exist");
            }
        }

        let before = self.clone();
        let mut next = self.clone();
        next.default_profile_id = id.map(str::to_owned);

        next.validate()?;
        let mutation = ProfileStoreMutation::from_stores(&before, &next);
        *self = next;
        Ok(mutation)
    }
}

impl ProfileStoreMutation {
    fn from_stores(before: &ProfileStoreV1, after: &ProfileStoreV1) -> Self {
        Self {
            changed: before != after,
            profile_count: after.profiles.len(),
            default_profile_changed: before.default_profile_id != after.default_profile_id,
        }
    }
}

impl OpenSshImportPreview {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn total_warning_count(&self) -> usize {
        self.warnings.len()
            + self
                .candidates
                .iter()
                .map(|candidate| candidate.warnings.len())
                .sum::<usize>()
    }

    pub fn conflict_count(&self) -> usize {
        self.candidates
            .iter()
            .filter(|candidate| candidate.conflict.is_some())
            .count()
    }

    pub fn mark_conflicts_from_store(&mut self, store: &ProfileStoreV1) -> usize {
        let existing_ids: BTreeSet<_> = store
            .profiles
            .iter()
            .map(|profile| profile.id.clone())
            .collect();

        for candidate in &mut self.candidates {
            candidate.conflict = existing_ids.contains(&candidate.profile.id).then(|| {
                OpenSshImportConflict::ExistingProfileId {
                    profile_id: candidate.profile.id.clone(),
                }
            });
        }

        self.conflict_count()
    }

    pub fn redacted_review(&self) -> OpenSshImportReview {
        let id_counts =
            self.candidates
                .iter()
                .fold(BTreeMap::<&str, usize>::new(), |mut counts, candidate| {
                    *counts.entry(candidate.profile.id.as_str()).or_default() += 1;
                    counts
                });
        let selected_by_default = self
            .candidates
            .iter()
            .filter(|candidate| {
                candidate.conflict.is_none()
                    && id_counts
                        .get(candidate.profile.id.as_str())
                        .is_some_and(|count| *count == 1)
            })
            .map(|candidate| candidate.profile.id.clone())
            .collect();

        OpenSshImportReview {
            candidates: self
                .candidates
                .iter()
                .map(|candidate| OpenSshImportCandidateSummary {
                    id: candidate.profile.id.clone(),
                    name: candidate.profile.name.clone(),
                    tags: candidate.profile.tags.clone(),
                    warning_count: candidate.warnings.len(),
                    has_conflict: candidate.conflict.is_some(),
                })
                .collect(),
            selected_by_default,
            warning_count: self.total_warning_count(),
            global_warning_count: self.warnings.len(),
            conflict_count: self.conflict_count(),
        }
    }
}

impl OpenSshImportConflictPolicy {
    pub fn parse_cli_value(value: &str) -> Result<Self> {
        match value {
            "reject" => Ok(Self::Reject),
            "replace" => Ok(Self::Replace),
            _ => bail!("OpenSSH import conflict policy must be reject or replace"),
        }
    }

    pub fn as_cli_value(self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::Replace => "replace",
        }
    }
}

impl OpenSshImportSelection {
    pub fn all() -> Self {
        Self::All
    }

    pub fn profile_ids(ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::ProfileIds(ids.into_iter().map(Into::into).collect())
    }
}

impl OpenSshImportApplyReport {
    pub fn total_warning_count(&self) -> usize {
        self.candidate_warning_count + self.global_warning_count
    }
}

impl OpenSshImportCandidate {
    pub fn new(profile: SshProfile, source: OpenSshImportSource) -> Self {
        Self {
            profile,
            source,
            warnings: Vec::new(),
            conflict: None,
        }
    }
}

impl OpenSshImportSource {
    pub fn new(host_pattern: impl Into<String>) -> Self {
        Self {
            config_path: None,
            host_pattern: host_pattern.into(),
            line: None,
        }
    }
}

pub fn apply_openssh_import_preview(
    store: &mut ProfileStoreV1,
    preview: &OpenSshImportPreview,
    selection: &OpenSshImportSelection,
    conflict_policy: OpenSshImportConflictPolicy,
) -> Result<OpenSshImportApplyReport> {
    let selected_indexes = selected_openssh_import_candidate_indexes(preview, selection)?;
    if selected_indexes.is_empty() {
        bail!("OpenSSH import selection is empty");
    }

    let existing_ids: BTreeSet<_> = store
        .profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect();
    let conflict_count = selected_indexes
        .iter()
        .filter(|index| existing_ids.contains(&preview.candidates[**index].profile.id))
        .count();
    if conflict_policy == OpenSshImportConflictPolicy::Reject && conflict_count > 0 {
        bail!("OpenSSH import has {conflict_count} selected profile id conflicts");
    }

    let before = store.clone();
    let mut next = store.clone();
    let mut added = 0;
    let mut replaced = 0;
    let mut candidate_warning_count = 0;

    for index in selected_indexes {
        let candidate = &preview.candidates[index];
        candidate_warning_count += candidate.warnings.len();
        if existing_ids.contains(&candidate.profile.id) {
            next.update_profile(&candidate.profile.id, candidate.profile.clone())?;
            replaced += 1;
        } else {
            next.add_profile(
                candidate.profile.clone(),
                ProfileStoreDefaultPolicy::Preserve,
            )?;
            added += 1;
        }
    }

    next.validate()?;
    let mutation = ProfileStoreMutation::from_stores(&before, &next);
    *store = next;
    Ok(OpenSshImportApplyReport {
        mutation,
        selected: added + replaced,
        added,
        replaced,
        candidate_warning_count,
        global_warning_count: preview.warnings.len(),
    })
}

fn selected_openssh_import_candidate_indexes(
    preview: &OpenSshImportPreview,
    selection: &OpenSshImportSelection,
) -> Result<Vec<usize>> {
    match selection {
        OpenSshImportSelection::All => select_all_openssh_import_candidate_indexes(preview),
        OpenSshImportSelection::ProfileIds(ids) => {
            select_requested_openssh_import_candidate_indexes(preview, ids)
        }
    }
}

fn select_all_openssh_import_candidate_indexes(
    preview: &OpenSshImportPreview,
) -> Result<Vec<usize>> {
    let mut selected_ids = BTreeSet::new();
    let mut indexes = Vec::new();
    for (index, candidate) in preview.candidates.iter().enumerate() {
        if !selected_ids.insert(candidate.profile.id.as_str()) {
            bail!("OpenSSH import selection contains duplicate candidate profile ids");
        }
        indexes.push(index);
    }
    Ok(indexes)
}

fn select_requested_openssh_import_candidate_indexes(
    preview: &OpenSshImportPreview,
    ids: &[String],
) -> Result<Vec<usize>> {
    let mut requested_ids = BTreeSet::new();
    for id in ids {
        validate_ssh_atom("import profile id", id)?;
        if !requested_ids.insert(id.as_str()) {
            bail!("OpenSSH import selection contains duplicate requested profile ids");
        }
    }

    let mut matched_ids = BTreeSet::new();
    let mut indexes = Vec::new();
    for (index, candidate) in preview.candidates.iter().enumerate() {
        if requested_ids.contains(candidate.profile.id.as_str()) {
            if !matched_ids.insert(candidate.profile.id.as_str()) {
                bail!("OpenSSH import selection contains duplicate candidate profile ids");
            }
            indexes.push(index);
        }
    }

    if matched_ids.len() != requested_ids.len() {
        bail!("OpenSSH import selection contains unknown profile ids");
    }
    Ok(indexes)
}

pub fn parse_openssh_import_preview(
    config: &str,
    config_path: Option<PathBuf>,
) -> OpenSshImportPreview {
    let mut preview = OpenSshImportPreview::new();
    let mut current = None;

    for (line_index, line) in config.lines().enumerate() {
        let tokens = tokenize_openssh_config_line(line);
        let Some((keyword, args)) = split_openssh_keyword(tokens) else {
            continue;
        };
        let normalized_keyword = keyword.to_ascii_lowercase();
        let line_number = line_index + 1;

        if normalized_keyword == "host" {
            if let Some(block) = current.take() {
                finish_openssh_import_block(&mut preview, block, &config_path);
            }
            if args.is_empty() {
                preview
                    .warnings
                    .push(OpenSshImportWarning::InvalidDirectiveValue {
                        keyword,
                        value: String::new(),
                    });
                continue;
            }
            current = Some(OpenSshImportBlock {
                patterns: args,
                line: Some(line_number),
                ..OpenSshImportBlock::default()
            });
            continue;
        }

        if normalized_keyword == "include" {
            push_include_warnings(&mut preview.warnings, &keyword, &args);
            continue;
        }

        let Some(block) = current.as_mut() else {
            preview
                .warnings
                .push(OpenSshImportWarning::UnsupportedDirective { keyword });
            continue;
        };

        apply_openssh_import_directive(block, keyword, normalized_keyword.as_str(), args);
    }

    if let Some(block) = current.take() {
        finish_openssh_import_block(&mut preview, block, &config_path);
    }

    preview
}

#[derive(Clone, Debug, Default)]
struct OpenSshImportBlock {
    patterns: Vec<String>,
    line: Option<usize>,
    host_name: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<PathBuf>,
    proxy_jump: Option<String>,
    request_tty: Option<bool>,
    term: Option<String>,
    remote_command: Vec<String>,
    warnings: Vec<OpenSshImportWarning>,
}

fn apply_openssh_import_directive(
    block: &mut OpenSshImportBlock,
    keyword: String,
    normalized_keyword: &str,
    args: Vec<String>,
) {
    match normalized_keyword {
        "hostname" => set_single_string(&mut block.host_name, &mut block.warnings, keyword, args),
        "user" => set_single_string(&mut block.user, &mut block.warnings, keyword, args),
        "port" => set_port(block, keyword, args),
        "identityfile" => set_identity_file(block, keyword, args),
        "proxyjump" => set_single_string(&mut block.proxy_jump, &mut block.warnings, keyword, args),
        "requesttty" => set_request_tty(block, keyword, args),
        "setenv" => set_env_term(block, keyword, args),
        "remotecommand" => set_remote_command(block, keyword, args),
        "proxycommand" => block
            .warnings
            .push(OpenSshImportWarning::ProxyCommandNotImported),
        _ => block
            .warnings
            .push(OpenSshImportWarning::UnsupportedDirective { keyword }),
    }
}

fn set_single_string(
    target: &mut Option<String>,
    warnings: &mut Vec<OpenSshImportWarning>,
    keyword: String,
    args: Vec<String>,
) {
    if args.len() == 1 && !args[0].is_empty() {
        *target = Some(args[0].clone());
    } else {
        warnings.push(OpenSshImportWarning::InvalidDirectiveValue {
            keyword,
            value: args.join(" "),
        });
    }
}

fn set_port(block: &mut OpenSshImportBlock, keyword: String, args: Vec<String>) {
    if args.len() == 1 {
        if let Ok(port) = args[0].parse::<u16>() {
            block.port = Some(port);
            return;
        }
    }
    block
        .warnings
        .push(OpenSshImportWarning::InvalidDirectiveValue {
            keyword,
            value: args.join(" "),
        });
}

fn set_identity_file(block: &mut OpenSshImportBlock, keyword: String, args: Vec<String>) {
    if args.len() == 1 && !args[0].is_empty() {
        block.identity_file = Some(PathBuf::from(&args[0]));
    } else {
        block
            .warnings
            .push(OpenSshImportWarning::InvalidDirectiveValue {
                keyword,
                value: args.join(" "),
            });
    }
}

fn set_request_tty(block: &mut OpenSshImportBlock, keyword: String, args: Vec<String>) {
    if args.len() == 1 {
        match args[0].to_ascii_lowercase().as_str() {
            "yes" | "force" => {
                block.request_tty = Some(true);
                return;
            }
            "no" | "auto" => {
                block.request_tty = Some(false);
                return;
            }
            _ => {}
        }
    }
    block
        .warnings
        .push(OpenSshImportWarning::InvalidDirectiveValue {
            keyword,
            value: args.join(" "),
        });
}

fn set_env_term(block: &mut OpenSshImportBlock, keyword: String, args: Vec<String>) {
    let term_values: Vec<_> = args
        .iter()
        .filter_map(|arg| arg.strip_prefix("TERM="))
        .collect();
    if term_values.len() == 1 && !term_values[0].is_empty() {
        block.term = Some(term_values[0].to_owned());
    } else if term_values.is_empty() {
        block
            .warnings
            .push(OpenSshImportWarning::UnsupportedDirective { keyword });
    } else {
        block
            .warnings
            .push(OpenSshImportWarning::InvalidDirectiveValue {
                keyword,
                value: args.join(" "),
            });
    }
}

fn set_remote_command(block: &mut OpenSshImportBlock, keyword: String, args: Vec<String>) {
    if args.is_empty() {
        block
            .warnings
            .push(OpenSshImportWarning::InvalidDirectiveValue {
                keyword,
                value: String::new(),
            });
        return;
    }
    block.remote_command = args;
    block
        .warnings
        .push(OpenSshImportWarning::RemoteCommandImported);
}

fn push_include_warnings(warnings: &mut Vec<OpenSshImportWarning>, keyword: &str, args: &[String]) {
    if args.is_empty() {
        warnings.push(OpenSshImportWarning::InvalidDirectiveValue {
            keyword: keyword.to_owned(),
            value: String::new(),
        });
        return;
    }
    warnings.extend(
        args.iter()
            .cloned()
            .map(|path| OpenSshImportWarning::IncludeNotExpanded { path }),
    );
}

fn finish_openssh_import_block(
    preview: &mut OpenSshImportPreview,
    block: OpenSshImportBlock,
    config_path: &Option<PathBuf>,
) {
    for pattern in block.patterns {
        if pattern.starts_with('!') {
            preview
                .warnings
                .push(OpenSshImportWarning::UnsupportedPattern { pattern });
            continue;
        }
        if pattern.contains('*') || pattern.contains('?') {
            preview
                .warnings
                .push(OpenSshImportWarning::SkippedWildcardOnlyHost { pattern });
            continue;
        }
        if pattern.chars().any(char::is_whitespace) || pattern.chars().any(char::is_control) {
            preview
                .warnings
                .push(OpenSshImportWarning::UnsupportedPattern { pattern });
            continue;
        }

        let Some(profile_id) = profile_id_from_host_pattern(&pattern) else {
            preview.warnings.push(OpenSshImportWarning::EmptyProfileId {
                host_pattern: pattern,
            });
            continue;
        };

        let mut warnings = block.warnings.clone();
        let target_host = match &block.host_name {
            Some(host_name) => host_name.clone(),
            None => {
                warnings.push(OpenSshImportWarning::MissingHostName {
                    host_pattern: pattern.clone(),
                });
                pattern.clone()
            }
        };

        push_token_warning(&mut warnings, "HostName", &target_host);
        push_optional_token_warning(&mut warnings, "User", block.user.as_deref());
        push_optional_token_warning(&mut warnings, "ProxyJump", block.proxy_jump.as_deref());
        push_optional_token_warning(&mut warnings, "TERM", block.term.as_deref());
        if block
            .remote_command
            .iter()
            .any(|arg| has_unexpanded_openssh_token(arg))
        {
            warnings.push(OpenSshImportWarning::TokenExpansionPreserved {
                field: "RemoteCommand".to_owned(),
            });
        }

        let mut profile = SshProfile::new(profile_id, pattern.clone(), target_host);
        profile.description("Imported from OpenSSH config");
        profile.tag("imported").tag("openssh");
        profile.target.user = block.user.clone();
        profile.target.port = block.port;
        profile.target.jump_host = block.proxy_jump.clone();
        profile.terminal.request_tty = block.request_tty.unwrap_or(true);
        if let Some(term) = &block.term {
            profile.terminal.term = Some(term.clone());
        }
        if let Some(identity_file) = &block.identity_file {
            profile.credential = SshCredentialRef::IdentityFile {
                path: identity_file.clone(),
            };
            warnings.push(OpenSshImportWarning::IdentityFilePathOnly);
            push_token_warning(
                &mut warnings,
                "IdentityFile",
                &identity_file.to_string_lossy(),
            );
        }
        profile.openssh.remote_command = block.remote_command.clone();

        if profile.launchability().is_err() {
            preview
                .warnings
                .push(OpenSshImportWarning::UnsupportedPattern { pattern });
            continue;
        }

        let mut candidate = OpenSshImportCandidate::new(
            profile,
            OpenSshImportSource {
                config_path: config_path.clone(),
                host_pattern: pattern,
                line: block.line,
            },
        );
        candidate.warnings = warnings;
        preview.candidates.push(candidate);
    }
}

fn profile_id_from_host_pattern(pattern: &str) -> Option<String> {
    let mut id = String::new();
    let mut last_was_separator = false;
    for ch in pattern.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if matches!(ch, '-' | '_' | '.') {
            Some(ch)
        } else {
            Some('-')
        };

        if let Some(ch) = next {
            let is_separator = matches!(ch, '-' | '_' | '.');
            if is_separator && last_was_separator {
                continue;
            }
            id.push(ch);
            last_was_separator = is_separator;
        }
    }

    let trimmed = id.trim_matches(['-', '_', '.']).to_owned();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn push_optional_token_warning(
    warnings: &mut Vec<OpenSshImportWarning>,
    field: &str,
    value: Option<&str>,
) {
    if let Some(value) = value {
        push_token_warning(warnings, field, value);
    }
}

fn push_token_warning(warnings: &mut Vec<OpenSshImportWarning>, field: &str, value: &str) {
    if has_unexpanded_openssh_token(value) {
        warnings.push(OpenSshImportWarning::TokenExpansionPreserved {
            field: field.to_owned(),
        });
    }
}

fn has_unexpanded_openssh_token(value: &str) -> bool {
    value.starts_with('~') || value.contains('%') || value.contains('$')
}

fn tokenize_openssh_config_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut token_started = false;
    let mut quote = None;
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            token.push(ch);
            token_started = true;
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            token_started = true;
            continue;
        }

        if let Some(quote_char) = quote {
            if ch == quote_char {
                quote = None;
            } else {
                token.push(ch);
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            token_started = true;
            continue;
        }

        if ch == '#' && !token_started {
            break;
        }

        if ch.is_whitespace() {
            if token_started {
                tokens.push(std::mem::take(&mut token));
                token_started = false;
            }
            continue;
        }

        token.push(ch);
        token_started = true;
    }

    if escaped {
        token.push('\\');
    }
    if token_started {
        tokens.push(token);
    }

    tokens
}

fn split_openssh_keyword(tokens: Vec<String>) -> Option<(String, Vec<String>)> {
    let mut tokens = tokens.into_iter();
    let first = tokens.next()?;
    let mut args = Vec::new();
    if let Some((keyword, value)) = first.split_once('=') {
        if !value.is_empty() {
            args.push(value.to_owned());
        }
        args.extend(tokens);
        Some((keyword.to_owned(), args))
    } else {
        args.extend(tokens);
        Some((first, args))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_profile_store(path: &Path) -> Result<ProfileStoreV1> {
    let json = fs::read_to_string(path)
        .with_context(|| format!("read profile store {}", path.display()))?;
    ProfileStoreV1::from_json(&json)
        .with_context(|| format!("parse profile store {}", path.display()))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn default_profile_store_path() -> Result<PathBuf> {
    default_profile_store_path_from_env(|name| std::env::var_os(name))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn edit_profile_store(
    path: &Path,
    mode: ProfileStoreEditOpenMode,
    edit: impl FnOnce(&mut ProfileStoreV1) -> Result<ProfileStoreMutation>,
) -> Result<ProfileStoreEditReport> {
    let parent = profile_store_parent(path)?;
    let file_name = profile_store_file_name(path)?;
    let target_exists = path
        .try_exists()
        .with_context(|| format!("check profile store target {}", path.display()))?;
    if !target_exists && mode == ProfileStoreEditOpenMode::Existing {
        bail!("profile store does not exist: {}", path.display());
    }

    let created_parent_dir = ensure_profile_store_parent_dir(parent)?;
    let _lock_guard = acquire_profile_store_write_lock(parent, file_name)?;
    let mut store = if path
        .try_exists()
        .with_context(|| format!("check profile store target {}", path.display()))?
    {
        read_profile_store(path)?
    } else {
        match mode {
            ProfileStoreEditOpenMode::Existing => {
                bail!("profile store does not exist: {}", path.display());
            }
            ProfileStoreEditOpenMode::CreateIfMissing => ProfileStoreV1::new(),
        }
    };

    let mutation =
        edit(&mut store).with_context(|| format!("edit profile store {}", path.display()))?;
    store
        .validate()
        .with_context(|| format!("validate edited profile store {}", path.display()))?;

    let write =
        write_profile_store_atomic_locked(path, parent, file_name, &store, created_parent_dir)?;
    Ok(ProfileStoreEditReport { write, mutation })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn write_profile_store_atomic(
    path: &Path,
    store: &ProfileStoreV1,
) -> Result<ProfileStoreWriteReport> {
    let parent = profile_store_parent(path)?;
    let file_name = profile_store_file_name(path)?;
    let json = profile_store_json(path, store)?;

    let created_parent_dir = ensure_profile_store_parent_dir(parent)?;
    let _lock_guard = acquire_profile_store_write_lock(parent, file_name)?;
    write_profile_store_prepared(path, parent, file_name, &json, created_parent_dir)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_profile_store_atomic_locked(
    path: &Path,
    parent: &Path,
    file_name: &OsStr,
    store: &ProfileStoreV1,
    created_parent_dir: bool,
) -> Result<ProfileStoreWriteReport> {
    let json = profile_store_json(path, store)?;
    write_profile_store_prepared(path, parent, file_name, &json, created_parent_dir)
}

#[cfg(not(target_arch = "wasm32"))]
fn profile_store_json(path: &Path, store: &ProfileStoreV1) -> Result<String> {
    let json = store
        .to_pretty_json()
        .with_context(|| format!("validate profile store before writing {}", path.display()))?;
    if json.len() > PROFILE_STORE_MAX_JSON_BYTES {
        bail!(
            "profile store JSON exceeds {} bytes",
            PROFILE_STORE_MAX_JSON_BYTES
        );
    }
    Ok(json)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_profile_store_prepared(
    path: &Path,
    parent: &Path,
    file_name: &OsStr,
    json: &str,
    created_parent_dir: bool,
) -> Result<ProfileStoreWriteReport> {
    let bytes = json.as_bytes();
    let (temp_path, mut temp_file) = create_profile_store_temp_file(parent, file_name)?;
    let write_result = (|| -> Result<()> {
        temp_file
            .write_all(bytes)
            .with_context(|| format!("write temporary profile store {}", temp_path.display()))?;
        temp_file
            .flush()
            .with_context(|| format!("flush temporary profile store {}", temp_path.display()))?;
        temp_file
            .sync_all()
            .with_context(|| format!("sync temporary profile store {}", temp_path.display()))?;
        drop(temp_file);

        replace_profile_store(&temp_path, path)?;
        sync_profile_store_parent_dir(parent);
        Ok(())
    })();

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(ProfileStoreWriteReport {
        path: path.to_path_buf(),
        bytes_written: bytes.len(),
        created_parent_dir,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn profile_store_parent(path: &Path) -> Result<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("profile store path must have a parent directory")
}

#[cfg(not(target_arch = "wasm32"))]
fn profile_store_file_name(path: &Path) -> Result<&OsStr> {
    path.file_name()
        .filter(|name| !name.is_empty())
        .context("profile store path must have a file name")
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "macos"))]
fn default_profile_store_path_from_env(
    env_var: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    let home = env_path("HOME", &env_var).context("HOME is not set for profile store path")?;
    Ok(home
        .join("Library")
        .join("Application Support")
        .join("Witty")
        .join("profiles.v1.json"))
}

#[cfg(all(not(target_arch = "wasm32"), windows))]
fn default_profile_store_path_from_env(
    env_var: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    let appdata =
        env_path("APPDATA", &env_var).context("APPDATA is not set for profile store path")?;
    Ok(appdata.join("Witty").join("profiles.v1.json"))
}

#[cfg(all(not(target_arch = "wasm32"), unix, not(target_os = "macos")))]
fn default_profile_store_path_from_env(
    env_var: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    if let Some(config_home) = env_path("XDG_CONFIG_HOME", &env_var) {
        if config_home.is_absolute() {
            return Ok(config_home.join("witty").join("profiles.v1.json"));
        }
    }

    let home = env_path("HOME", &env_var).context("HOME is not set for profile store path")?;
    Ok(home.join(".config").join("witty").join("profiles.v1.json"))
}

#[cfg(all(not(target_arch = "wasm32"), not(unix), not(windows)))]
fn default_profile_store_path_from_env(
    _env_var: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    bail!("default profile store path is not supported on this platform")
}

#[cfg(not(target_arch = "wasm32"))]
fn env_path(name: &str, env_var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    env_var(name)
        .filter(|value| !value.as_os_str().is_empty())
        .map(PathBuf::from)
}

#[cfg(not(target_arch = "wasm32"))]
fn ensure_profile_store_parent_dir(parent: &Path) -> Result<bool> {
    if parent
        .try_exists()
        .with_context(|| format!("check profile store parent {}", parent.display()))?
    {
        if !parent.is_dir() {
            bail!(
                "profile store parent {} is not a directory",
                parent.display()
            );
        }
        return Ok(false);
    }

    fs::create_dir_all(parent)
        .with_context(|| format!("create profile store parent {}", parent.display()))?;
    set_created_profile_store_parent_permissions(parent)?;
    Ok(true)
}

#[cfg(all(not(target_arch = "wasm32"), unix))]
fn set_created_profile_store_parent_permissions(parent: &Path) -> Result<()> {
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).with_context(|| {
        format!(
            "set profile store parent permissions to 0700 for {}",
            parent.display()
        )
    })
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn set_created_profile_store_parent_permissions(_parent: &Path) -> Result<()> {
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
struct ProfileStoreWriteLock {
    path: PathBuf,
    marker: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for ProfileStoreWriteLock {
    fn drop(&mut self) {
        if matches!(
            fs::read_to_string(&self.path),
            Ok(marker) if marker == self.marker
        ) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn acquire_profile_store_write_lock(
    parent: &Path,
    file_name: &OsStr,
) -> Result<ProfileStoreWriteLock> {
    let lock_path = profile_store_lock_path(parent, file_name);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = match options.open(&lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!(
                "profile store write lock already exists: {}",
                lock_path.display()
            );
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("create profile store write lock {}", lock_path.display())
            });
        }
    };

    let marker_counter = PROFILE_STORE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let marker = format!("pid={}\ncounter={marker_counter}\n", std::process::id());
    let guard = ProfileStoreWriteLock {
        path: lock_path,
        marker,
    };
    let write_result = (|| -> Result<()> {
        set_profile_store_file_permissions(&guard.path)?;
        file.write_all(guard.marker.as_bytes())
            .with_context(|| format!("write profile store lock marker {}", guard.path.display()))?;
        file.flush()
            .with_context(|| format!("flush profile store lock {}", guard.path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync profile store lock {}", guard.path.display()))?;
        Ok(())
    })();

    if let Err(error) = write_result {
        drop(guard);
        return Err(error);
    }

    Ok(guard)
}

#[cfg(not(target_arch = "wasm32"))]
fn profile_store_lock_path(parent: &Path, file_name: &OsStr) -> PathBuf {
    let mut lock_name = OsString::from(file_name);
    lock_name.push(".lock");
    parent.join(lock_name)
}

#[cfg(not(target_arch = "wasm32"))]
fn create_profile_store_temp_file(parent: &Path, file_name: &OsStr) -> Result<(PathBuf, fs::File)> {
    for _ in 0..PROFILE_STORE_TEMP_FILE_RETRIES {
        let temp_path = profile_store_temp_path(parent, file_name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);

        match options.open(&temp_path) {
            Ok(file) => {
                set_profile_store_file_permissions(&temp_path)?;
                return Ok((temp_path, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "create temporary profile store file {}",
                        temp_path.display()
                    )
                });
            }
        }
    }

    bail!(
        "could not create a unique temporary profile store file in {} after {} attempts",
        parent.display(),
        PROFILE_STORE_TEMP_FILE_RETRIES
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn profile_store_temp_path(parent: &Path, file_name: &OsStr) -> PathBuf {
    let counter = PROFILE_STORE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut temp_name = OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".tmp.{}.{}", std::process::id(), counter));
    parent.join(temp_name)
}

#[cfg(all(not(target_arch = "wasm32"), unix))]
fn set_profile_store_file_permissions(path: &Path) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "set profile store file permissions to 0600 for {}",
            path.display()
        )
    })
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn set_profile_store_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), not(windows)))]
fn replace_profile_store(temp_path: &Path, target_path: &Path) -> Result<()> {
    fs::rename(temp_path, target_path).with_context(|| {
        format!(
            "atomically replace profile store {} with {}",
            target_path.display(),
            temp_path.display()
        )
    })
}

#[cfg(all(not(target_arch = "wasm32"), windows))]
fn replace_profile_store(temp_path: &Path, target_path: &Path) -> Result<()> {
    if target_path
        .try_exists()
        .with_context(|| format!("check profile store target {}", target_path.display()))?
    {
        bail!(
            "atomic profile store replacement is not implemented for existing Windows targets: {}",
            target_path.display()
        );
    }
    fs::rename(temp_path, target_path).with_context(|| {
        format!(
            "create profile store {} from {}",
            target_path.display(),
            temp_path.display()
        )
    })
}

#[cfg(all(not(target_arch = "wasm32"), unix))]
fn sync_profile_store_parent_dir(parent: &Path) {
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
fn sync_profile_store_parent_dir(_parent: &Path) {}

impl SshProfile {
    pub fn new(id: impl Into<String>, name: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            tags: Vec::new(),
            target: SshProfileTarget::new(host),
            credential: SshCredentialRef::DefaultAgent,
            terminal: SshTerminalOptions::default(),
            openssh: OpenSshAdvancedOptions::default(),
        }
    }

    pub fn tag(&mut self, tag: impl Into<String>) -> &mut Self {
        self.tags.push(tag.into());
        self
    }

    pub fn description(&mut self, description: impl Into<String>) -> &mut Self {
        self.description = Some(description.into());
        self
    }

    pub fn to_openssh_profile(&self) -> Result<OpenSshProfile> {
        self.validate_product_fields()?;

        let mut profile = OpenSshProfile::new(self.target.host.clone());
        if let Some(user) = &self.target.user {
            profile.user(user.clone());
        }
        if let Some(port) = self.target.port {
            profile.port(port);
        }
        if let Some(jump_host) = &self.target.jump_host {
            profile.jump_host(jump_host.clone());
        }

        match &self.credential {
            SshCredentialRef::DefaultAgent => {}
            SshCredentialRef::IdentityFile { path } => {
                profile.identity_file(path.clone());
            }
            SshCredentialRef::VaultSecret { secret_id } => {
                validate_ssh_atom("vault secret id", secret_id)?;
                bail!("vault credential references require a credential resolver");
            }
        }

        match &self.terminal.term {
            Some(term) => {
                profile.term(term.clone());
            }
            None => {
                profile.no_term_env();
            }
        }
        profile.request_tty(self.terminal.request_tty);

        if let Some(config_file) = &self.openssh.config_file {
            profile.config_file(config_file.clone());
        }
        profile.extra_args(self.openssh.extra_args.iter().cloned());
        profile.remote_command(self.openssh.remote_command.iter().cloned());

        profile.destination()?;
        validate_optional_ssh_atom("jump host", profile.jump_host.as_deref())?;
        validate_optional_ssh_atom("TERM", profile.term.as_deref())?;
        Ok(profile)
    }

    pub fn launchability(&self) -> Result<SshProfileLaunchability> {
        self.validate_product_fields()?;
        match &self.credential {
            SshCredentialRef::DefaultAgent | SshCredentialRef::IdentityFile { .. } => {
                self.to_openssh_profile()?;
                Ok(SshProfileLaunchability::Launchable)
            }
            SshCredentialRef::VaultSecret { secret_id } => {
                validate_ssh_atom("vault secret id", secret_id)?;
                Ok(SshProfileLaunchability::RequiresCredentialResolver)
            }
        }
    }

    fn validate_product_fields(&self) -> Result<()> {
        validate_ssh_atom("profile id", &self.id)?;
        validate_label("profile name", &self.name)?;
        if let Some(description) = &self.description {
            validate_text("profile description", description)?;
        }
        for tag in &self.tags {
            validate_ssh_atom("profile tag", tag)?;
        }
        validate_ssh_atom("host", &self.target.host)?;
        validate_optional_ssh_atom("user", self.target.user.as_deref())?;
        validate_optional_ssh_atom("jump host", self.target.jump_host.as_deref())?;
        validate_optional_ssh_atom("TERM", self.terminal.term.as_deref())?;
        Ok(())
    }
}

impl SshProfileTarget {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            user: None,
            port: None,
            jump_host: None,
        }
    }

    pub fn user(&mut self, user: impl Into<String>) -> &mut Self {
        self.user = Some(user.into());
        self
    }

    pub fn port(&mut self, port: u16) -> &mut Self {
        self.port = Some(port);
        self
    }

    pub fn jump_host(&mut self, jump_host: impl Into<String>) -> &mut Self {
        self.jump_host = Some(jump_host.into());
        self
    }
}

impl Default for SshTerminalOptions {
    fn default() -> Self {
        Self {
            term: Some("xterm-256color".to_owned()),
            request_tty: true,
        }
    }
}

impl OpenSshProfile {
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            user: None,
            port: None,
            identity_file: None,
            config_file: None,
            jump_host: None,
            term: Some("xterm-256color".to_owned()),
            request_tty: true,
            extra_args: Vec::new(),
            remote_command: Vec::new(),
        }
    }

    pub fn user(&mut self, user: impl Into<String>) -> &mut Self {
        self.user = Some(user.into());
        self
    }

    pub fn port(&mut self, port: u16) -> &mut Self {
        self.port = Some(port);
        self
    }

    pub fn identity_file(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.identity_file = Some(path.into());
        self
    }

    pub fn config_file(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.config_file = Some(path.into());
        self
    }

    pub fn jump_host(&mut self, jump_host: impl Into<String>) -> &mut Self {
        self.jump_host = Some(jump_host.into());
        self
    }

    pub fn term(&mut self, term: impl Into<String>) -> &mut Self {
        self.term = Some(term.into());
        self
    }

    pub fn no_term_env(&mut self) -> &mut Self {
        self.term = None;
        self
    }

    pub fn request_tty(&mut self, request_tty: bool) -> &mut Self {
        self.request_tty = request_tty;
        self
    }

    pub fn extra_arg(&mut self, arg: impl Into<String>) -> &mut Self {
        self.extra_args.push(arg.into());
        self
    }

    pub fn extra_args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for arg in args {
            self.extra_arg(arg);
        }
        self
    }

    pub fn remote_command<I, S>(&mut self, command: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.remote_command = command.into_iter().map(Into::into).collect();
        self
    }

    pub fn destination(&self) -> Result<String> {
        validate_ssh_atom("host", &self.host)?;
        if let Some(user) = &self.user {
            validate_ssh_atom("user", user)?;
            Ok(format!("{user}@{}", self.host))
        } else {
            Ok(self.host.clone())
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn to_local_pty_config(&self, size: GridSize) -> Result<LocalPtyConfig> {
        let destination = self.destination()?;
        validate_optional_ssh_atom("jump host", self.jump_host.as_deref())?;
        validate_optional_ssh_atom("TERM", self.term.as_deref())?;

        let mut config = LocalPtyConfig::command(size, "ssh");
        if let Some(term) = &self.term {
            config.env("TERM", term);
        }
        config.args(self.args_before_destination());
        config.arg(destination);
        config.args(self.remote_command.iter().cloned());
        Ok(config)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn args_before_destination(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.request_tty {
            args.push("-tt".to_owned());
        }
        if let Some(port) = self.port {
            args.push("-p".to_owned());
            args.push(port.to_string());
        }
        if let Some(path) = &self.identity_file {
            args.push("-i".to_owned());
            args.push(path.to_string_lossy().into_owned());
        }
        if let Some(path) = &self.config_file {
            args.push("-F".to_owned());
            args.push(path.to_string_lossy().into_owned());
        }
        if let Some(jump_host) = &self.jump_host {
            args.push("-J".to_owned());
            args.push(jump_host.clone());
        }
        args.extend(self.extra_args.iter().cloned());
        args
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_openssh_config_dump_smoke() -> Result<OpenSshConfigDumpSmoke> {
    let size = GridSize::new(24, 80);
    let mut profile = OpenSshProfile::new("witty.invalid");
    profile
        .request_tty(false)
        .no_term_env()
        .config_file("none")
        .extra_args(["-G", "-o", "BatchMode=yes"]);

    let destination = profile.destination()?;
    let config = profile.to_local_pty_config(size)?;
    let mut transport = LocalPtyTransport::spawn(config).context("spawn ssh -G smoke pty")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = Vec::new();
    let mut exit_code = None;

    while Instant::now() < deadline {
        while let Some(event) = transport.poll_event()? {
            match event {
                TransportEvent::Output(bytes) => output.extend(bytes),
                TransportEvent::Exit { code } => {
                    exit_code = code;
                }
                TransportEvent::Error(err) => bail!("openssh config dump smoke error: {err}"),
            }
        }

        if exit_code.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    let Some(exit_code) = exit_code else {
        bail!("openssh config dump smoke timed out");
    };

    let output = String::from_utf8_lossy(&output).into_owned();
    if exit_code != 0 {
        bail!("openssh config dump smoke exited with {exit_code:?}: {output}");
    }
    if !output.contains("hostname witty.invalid") {
        bail!("openssh config dump smoke did not print expected hostname: {output}");
    }
    if !output.contains("batchmode yes") {
        bail!("openssh config dump smoke did not apply BatchMode=yes: {output}");
    }

    Ok(OpenSshConfigDumpSmoke {
        destination,
        exit_code: Some(exit_code),
        output,
    })
}

fn validate_optional_ssh_atom(label: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        validate_ssh_atom(label, value)?;
    }
    Ok(())
}

fn validate_label(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    validate_text(label, value)
}

fn validate_text(label: &str, value: &str) -> Result<()> {
    if value.chars().any(char::is_control) {
        bail!("{label} must not contain control characters");
    }
    Ok(())
}

fn validate_ssh_atom(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{label} must not be empty");
    }
    if value.chars().any(char::is_whitespace) {
        bail!("{label} must not contain whitespace");
    }
    if value.chars().any(char::is_control) {
        bail!("{label} must not contain control characters");
    }
    Ok(())
}

fn validate_profile_store_limits(profile: &SshProfile) -> Result<()> {
    if profile.tags.len() > PROFILE_STORE_MAX_TAGS_PER_PROFILE {
        bail!(
            "profile {} has {} tags; maximum is {}",
            profile.id,
            profile.tags.len(),
            PROFILE_STORE_MAX_TAGS_PER_PROFILE
        );
    }
    if profile.openssh.extra_args.len() > PROFILE_STORE_MAX_OPENSSH_EXTRA_ARGS {
        bail!(
            "profile {} has {} OpenSSH extra args; maximum is {}",
            profile.id,
            profile.openssh.extra_args.len(),
            PROFILE_STORE_MAX_OPENSSH_EXTRA_ARGS
        );
    }
    if profile.openssh.remote_command.len() > PROFILE_STORE_MAX_REMOTE_COMMAND_ARGS {
        bail!(
            "profile {} has {} remote command args; maximum is {}",
            profile.id,
            profile.openssh.remote_command.len(),
            PROFILE_STORE_MAX_REMOTE_COMMAND_ARGS
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openssh_profile_formats_destination() {
        assert_eq!(
            OpenSshProfile::new("example.com").destination().unwrap(),
            "example.com"
        );

        let mut profile = OpenSshProfile::new("example.com");
        profile.user("alice");
        assert_eq!(profile.destination().unwrap(), "alice@example.com");
    }

    #[test]
    fn openssh_profile_rejects_unsafe_destination_atoms() {
        assert!(OpenSshProfile::new("").destination().is_err());
        assert!(OpenSshProfile::new("bad host").destination().is_err());

        let mut profile = OpenSshProfile::new("example.com");
        profile.user("bad user");
        assert!(profile.destination().is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn openssh_profile_builds_local_pty_config() {
        let mut profile = OpenSshProfile::new("example.com");
        profile
            .user("alice")
            .port(2222)
            .identity_file("/tmp/id_ed25519")
            .config_file("/tmp/ssh_config")
            .jump_host("bastion")
            .term("xterm-witty")
            .extra_args(["-o", "ServerAliveInterval=30"])
            .remote_command(["tmux", "new", "-A", "-s", "main"]);

        let config = profile.to_local_pty_config(GridSize::new(40, 120)).unwrap();

        assert_eq!(config.size, GridSize::new(40, 120));
        assert_eq!(config.program.as_deref(), Some("ssh"));
        assert_eq!(
            config.args,
            vec![
                "-tt",
                "-p",
                "2222",
                "-i",
                "/tmp/id_ed25519",
                "-F",
                "/tmp/ssh_config",
                "-J",
                "bastion",
                "-o",
                "ServerAliveInterval=30",
                "alice@example.com",
                "tmux",
                "new",
                "-A",
                "-s",
                "main",
            ]
        );
        assert_eq!(
            config.env,
            vec![("TERM".to_owned(), "xterm-witty".to_owned())]
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn openssh_profile_can_disable_tty_and_term_env() {
        let mut profile = OpenSshProfile::new("example.com");
        profile.request_tty(false).no_term_env();

        let config = profile.to_local_pty_config(GridSize::new(24, 80)).unwrap();

        assert_eq!(config.args, vec!["example.com"]);
        assert!(config.env.is_empty());
    }

    #[cfg(all(not(target_arch = "wasm32"), unix))]
    #[test]
    fn openssh_config_dump_smoke_runs_without_network() {
        let smoke = run_openssh_config_dump_smoke().unwrap();

        assert_eq!(smoke.destination, "witty.invalid");
        assert_eq!(smoke.exit_code, Some(0));
        assert!(smoke.output.contains("hostname witty.invalid"));
        assert!(smoke.output.contains("batchmode yes"));
    }

    #[test]
    fn ssh_profile_schema_maps_metadata_target_credential_and_advanced_options() {
        let mut profile = SshProfile::new("prod-db", "Production DB", "db.example.com");
        profile
            .tag("prod")
            .description("Primary database bastion profile");
        profile
            .target
            .user("deploy")
            .port(2222)
            .jump_host("bastion.example.com");
        profile.credential = SshCredentialRef::IdentityFile {
            path: PathBuf::from("/home/alice/.ssh/prod_ed25519"),
        };
        profile.terminal.term = Some("xterm-witty".to_owned());
        profile.openssh.config_file = Some(PathBuf::from("/home/alice/.ssh/config"));
        profile.openssh.extra_args = vec!["-o".to_owned(), "ServerAliveInterval=30".to_owned()];
        profile.openssh.remote_command = vec!["tmux".to_owned(), "new".to_owned()];

        let openssh = profile.to_openssh_profile().unwrap();

        assert_eq!(openssh.destination().unwrap(), "deploy@db.example.com");
        assert_eq!(openssh.port, Some(2222));
        assert_eq!(openssh.jump_host.as_deref(), Some("bastion.example.com"));
        assert_eq!(
            openssh.identity_file,
            Some(PathBuf::from("/home/alice/.ssh/prod_ed25519"))
        );
        assert_eq!(
            openssh.config_file,
            Some(PathBuf::from("/home/alice/.ssh/config"))
        );
        assert_eq!(
            openssh.extra_args,
            vec!["-o".to_owned(), "ServerAliveInterval=30".to_owned()]
        );
        assert_eq!(
            openssh.remote_command,
            vec!["tmux".to_owned(), "new".to_owned()]
        );
    }

    #[test]
    fn ssh_profile_schema_rejects_unresolved_vault_credentials() {
        let mut profile = SshProfile::new("prod-db", "Production DB", "db.example.com");
        profile.credential = SshCredentialRef::VaultSecret {
            secret_id: "vault/prod/db".to_owned(),
        };

        assert!(profile.to_openssh_profile().is_err());
    }

    #[test]
    fn ssh_profile_schema_serializes_credential_references_without_secret_material() {
        let mut profile = SshProfile::new("prod-db", "Production DB", "db.example.com");
        profile.credential = SshCredentialRef::VaultSecret {
            secret_id: "vault-prod-db-key".to_owned(),
        };

        let json = serde_json::to_string(&profile).unwrap();

        assert!(json.contains("vault-prod-db-key"));
        assert!(!json.contains("passphrase"));
        assert!(!json.contains("private_key"));
    }

    #[test]
    fn ssh_profile_schema_validates_product_fields() {
        let mut profile = SshProfile::new("bad id", "Production DB", "db.example.com");
        assert!(profile.to_openssh_profile().is_err());

        profile.id = "prod-db".to_owned();
        profile.name = "\n".to_owned();
        assert!(profile.to_openssh_profile().is_err());

        profile.name = "Production DB".to_owned();
        profile.target.host = "bad host".to_owned();
        assert!(profile.to_openssh_profile().is_err());
    }

    #[test]
    fn openssh_import_preview_types_serialize_as_structured_data() {
        let mut candidate = OpenSshImportCandidate::new(
            SshProfile::new("prod-db", "Production DB", "db.example.com"),
            OpenSshImportSource {
                config_path: Some(PathBuf::from("/home/alice/.ssh/config")),
                host_pattern: "prod-db".to_owned(),
                line: Some(17),
            },
        );
        candidate
            .warnings
            .push(OpenSshImportWarning::IdentityFilePathOnly);
        candidate.conflict = Some(OpenSshImportConflict::ExistingProfileId {
            profile_id: "prod-db".to_owned(),
        });
        let preview = OpenSshImportPreview {
            candidates: vec![candidate],
            warnings: vec![OpenSshImportWarning::IncludeNotExpanded {
                path: "conf.d/*.conf".to_owned(),
            }],
        };

        let json = serde_json::to_string(&preview).unwrap();
        let decoded: OpenSshImportPreview = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, preview);
        assert!(json.contains("\"kind\":\"identity_file_path_only\""));
        assert!(json.contains("\"kind\":\"existing_profile_id\""));
        assert!(json.contains("\"kind\":\"include_not_expanded\""));
        assert_eq!(decoded.total_warning_count(), 2);
        assert_eq!(decoded.conflict_count(), 1);
    }

    #[test]
    fn openssh_import_preview_marks_conflicts_from_profile_store() {
        let store = profile_store_with("prod", "prod.example.com");
        let mut preview = OpenSshImportPreview {
            candidates: vec![
                OpenSshImportCandidate::new(
                    SshProfile::new("prod", "Production", "prod.example.com"),
                    OpenSshImportSource::new("prod"),
                ),
                OpenSshImportCandidate::new(
                    SshProfile::new("staging", "Staging", "staging.example.com"),
                    OpenSshImportSource::new("staging"),
                ),
            ],
            warnings: Vec::new(),
        };

        let conflict_count = preview.mark_conflicts_from_store(&store);

        assert_eq!(conflict_count, 1);
        assert_eq!(
            preview.candidates[0].conflict,
            Some(OpenSshImportConflict::ExistingProfileId {
                profile_id: "prod".to_owned(),
            })
        );
        assert_eq!(preview.candidates[1].conflict, None);
    }

    #[test]
    fn openssh_import_redacted_review_omits_sensitive_candidate_details() {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.target
            .user("alice")
            .port(2222)
            .jump_host("bastion.internal");
        prod.credential = SshCredentialRef::IdentityFile {
            path: PathBuf::from("/home/alice/.ssh/prod_ed25519"),
        };
        prod.openssh.config_file = Some(PathBuf::from("/home/alice/.ssh/config"));
        prod.openssh
            .extra_args
            .push("ServerAliveInterval=30".to_owned());
        prod.openssh.remote_command.push("uptime".to_owned());
        let mut prod_candidate = OpenSshImportCandidate::new(
            prod,
            OpenSshImportSource {
                config_path: Some(PathBuf::from("/home/alice/.ssh/config")),
                host_pattern: "prod".to_owned(),
                line: Some(17),
            },
        );
        prod_candidate
            .warnings
            .push(OpenSshImportWarning::IdentityFilePathOnly);
        prod_candidate.conflict = Some(OpenSshImportConflict::ExistingProfileId {
            profile_id: "prod".to_owned(),
        });

        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vault.example.com");
        vaulted.credential = SshCredentialRef::VaultSecret {
            secret_id: "vault-secret-prod".to_owned(),
        };
        let vaulted_candidate =
            OpenSshImportCandidate::new(vaulted, OpenSshImportSource::new("vaulted"));
        let staging = OpenSshImportCandidate::new(
            SshProfile::new("staging", "Staging", "stage.example.com"),
            OpenSshImportSource::new("staging"),
        );
        let duplicate_a = OpenSshImportCandidate::new(
            SshProfile::new("duplicate", "Duplicate A", "dup-a.example.com"),
            OpenSshImportSource::new("duplicate-a"),
        );
        let duplicate_b = OpenSshImportCandidate::new(
            SshProfile::new("duplicate", "Duplicate B", "dup-b.example.com"),
            OpenSshImportSource::new("duplicate-b"),
        );
        let preview = OpenSshImportPreview {
            candidates: vec![
                prod_candidate,
                vaulted_candidate,
                staging,
                duplicate_a,
                duplicate_b,
            ],
            warnings: vec![OpenSshImportWarning::IncludeNotExpanded {
                path: "/home/alice/.ssh/conf.d/*.conf".to_owned(),
            }],
        };

        let review = preview.redacted_review();
        let json = serde_json::to_string(&review).unwrap();

        assert_eq!(review.warning_count, 2);
        assert_eq!(review.global_warning_count, 1);
        assert_eq!(review.conflict_count, 1);
        assert_eq!(review.candidates.len(), 5);
        assert_eq!(
            review.selected_by_default,
            vec!["vaulted".to_owned(), "staging".to_owned()]
        );
        assert!(json.contains(r#""id":"prod""#));
        assert!(json.contains(r#""name":"Production""#));
        assert!(json.contains(r#""has_conflict":true"#));
        for sensitive in [
            "prod.example.com",
            "stage.example.com",
            "vault.example.com",
            "dup-a.example.com",
            "alice",
            "2222",
            "bastion.internal",
            "prod_ed25519",
            "/home/alice/.ssh/config",
            "/home/alice/.ssh/conf.d",
            "ServerAliveInterval",
            "uptime",
            "vault-secret-prod",
            "\"profile\"",
            "\"source\"",
            "\"target\"",
            "\"credential\"",
            "\"openssh\"",
            "\"host_pattern\"",
            "\"line\"",
        ] {
            assert!(
                !json.contains(sensitive),
                "redacted import review leaked {sensitive}: {json}"
            );
        }
    }

    #[test]
    fn openssh_import_redacted_review_rejects_unknown_fields() {
        let top_level = r#"{
            "candidates":[],
            "selected_by_default":[],
            "warning_count":0,
            "global_warning_count":0,
            "conflict_count":0,
            "config_path":"/home/user/.ssh/config"
        }"#;
        assert!(serde_json::from_str::<OpenSshImportReview>(top_level).is_err());

        let nested_candidate = r#"{
            "candidates":[{
                "id":"prod",
                "name":"Production",
                "tags":["work"],
                "warning_count":0,
                "has_conflict":false,
                "host":"prod.internal"
            }],
            "selected_by_default":["prod"],
            "warning_count":0,
            "global_warning_count":0,
            "conflict_count":0
        }"#;
        assert!(serde_json::from_str::<OpenSshImportReview>(nested_candidate).is_err());
    }

    #[test]
    fn openssh_import_conflict_policy_parses_cli_values() {
        assert_eq!(
            OpenSshImportConflictPolicy::parse_cli_value("reject").unwrap(),
            OpenSshImportConflictPolicy::Reject
        );
        assert_eq!(
            OpenSshImportConflictPolicy::parse_cli_value("replace").unwrap(),
            OpenSshImportConflictPolicy::Replace
        );
        assert_eq!(OpenSshImportConflictPolicy::Reject.as_cli_value(), "reject");
        assert!(OpenSshImportConflictPolicy::parse_cli_value("merge").is_err());
    }

    #[test]
    fn openssh_import_apply_adds_candidates_and_preserves_empty_default() {
        let mut preview = OpenSshImportPreview {
            candidates: vec![
                OpenSshImportCandidate::new(
                    SshProfile::new("prod", "Production", "prod.example.com"),
                    OpenSshImportSource::new("prod"),
                ),
                OpenSshImportCandidate::new(
                    SshProfile::new("staging", "Staging", "staging.example.com"),
                    OpenSshImportSource::new("staging"),
                ),
            ],
            warnings: vec![OpenSshImportWarning::IncludeNotExpanded {
                path: "conf.d/*.conf".to_owned(),
            }],
        };
        preview.candidates[0]
            .warnings
            .push(OpenSshImportWarning::IdentityFilePathOnly);
        let mut store = ProfileStoreV1::new();

        let report = apply_openssh_import_preview(
            &mut store,
            &preview,
            &OpenSshImportSelection::all(),
            OpenSshImportConflictPolicy::Reject,
        )
        .unwrap();

        assert_eq!(report.selected, 2);
        assert_eq!(report.added, 2);
        assert_eq!(report.replaced, 0);
        assert_eq!(report.candidate_warning_count, 1);
        assert_eq!(report.global_warning_count, 1);
        assert_eq!(report.total_warning_count(), 2);
        assert!(report.mutation.changed);
        assert_eq!(report.mutation.profile_count, 2);
        assert!(!report.mutation.default_profile_changed);
        assert_eq!(store.default_profile_id, None);
        assert_eq!(
            store.profile("prod").unwrap().target.host,
            "prod.example.com"
        );
        assert_eq!(
            store.profile("staging").unwrap().target.host,
            "staging.example.com"
        );
    }

    #[test]
    fn openssh_import_apply_reject_conflict_preserves_store() {
        let preview = OpenSshImportPreview {
            candidates: vec![
                OpenSshImportCandidate::new(
                    SshProfile::new("prod", "Production Imported", "new.example.com"),
                    OpenSshImportSource::new("prod"),
                ),
                OpenSshImportCandidate::new(
                    SshProfile::new("staging", "Staging", "staging.example.com"),
                    OpenSshImportSource::new("staging"),
                ),
            ],
            warnings: Vec::new(),
        };
        let mut store = profile_store_with("prod", "old.example.com");
        let before = store.clone();

        let error = apply_openssh_import_preview(
            &mut store,
            &preview,
            &OpenSshImportSelection::all(),
            OpenSshImportConflictPolicy::Reject,
        )
        .unwrap_err();

        assert!(error.to_string().contains("selected profile id conflicts"));
        assert_eq!(store, before);
    }

    #[test]
    fn openssh_import_apply_replace_updates_exact_ids_and_preserves_default() {
        let preview = OpenSshImportPreview {
            candidates: vec![
                OpenSshImportCandidate::new(
                    SshProfile::new("prod", "Production Imported", "new.example.com"),
                    OpenSshImportSource::new("prod"),
                ),
                OpenSshImportCandidate::new(
                    SshProfile::new("staging", "Staging", "staging.example.com"),
                    OpenSshImportSource::new("staging"),
                ),
            ],
            warnings: Vec::new(),
        };
        let mut store = ProfileStoreV1 {
            default_profile_id: Some("prod".to_owned()),
            ..ProfileStoreV1::with_profiles(vec![SshProfile::new(
                "prod",
                "Production",
                "old.example.com",
            )])
        };

        let report = apply_openssh_import_preview(
            &mut store,
            &preview,
            &OpenSshImportSelection::all(),
            OpenSshImportConflictPolicy::Replace,
        )
        .unwrap();

        assert_eq!(report.selected, 2);
        assert_eq!(report.added, 1);
        assert_eq!(report.replaced, 1);
        assert_eq!(report.mutation.profile_count, 2);
        assert!(!report.mutation.default_profile_changed);
        assert_eq!(store.default_profile_id.as_deref(), Some("prod"));
        assert_eq!(
            store.profile("prod").unwrap().target.host,
            "new.example.com"
        );
        assert_eq!(
            store.profile("staging").unwrap().target.host,
            "staging.example.com"
        );
    }

    #[test]
    fn openssh_import_apply_respects_selected_ids_and_rejects_bad_selection() {
        let preview = OpenSshImportPreview {
            candidates: vec![
                OpenSshImportCandidate::new(
                    SshProfile::new("prod", "Production", "prod.example.com"),
                    OpenSshImportSource::new("prod"),
                ),
                OpenSshImportCandidate::new(
                    SshProfile::new("staging", "Staging", "staging.example.com"),
                    OpenSshImportSource::new("staging"),
                ),
            ],
            warnings: Vec::new(),
        };
        let mut store = ProfileStoreV1::new();

        let report = apply_openssh_import_preview(
            &mut store,
            &preview,
            &OpenSshImportSelection::profile_ids(["staging"]),
            OpenSshImportConflictPolicy::Reject,
        )
        .unwrap();

        assert_eq!(report.selected, 1);
        assert!(store.profile("prod").is_none());
        assert!(store.profile("staging").is_some());

        assert!(apply_openssh_import_preview(
            &mut ProfileStoreV1::new(),
            &preview,
            &OpenSshImportSelection::profile_ids(["missing"]),
            OpenSshImportConflictPolicy::Reject,
        )
        .is_err());
        assert!(apply_openssh_import_preview(
            &mut ProfileStoreV1::new(),
            &preview,
            &OpenSshImportSelection::profile_ids(["prod", "prod"]),
            OpenSshImportConflictPolicy::Reject,
        )
        .is_err());
        assert!(apply_openssh_import_preview(
            &mut ProfileStoreV1::new(),
            &preview,
            &OpenSshImportSelection::profile_ids(Vec::<String>::new()),
            OpenSshImportConflictPolicy::Reject,
        )
        .is_err());
    }

    #[test]
    fn openssh_import_apply_rejects_duplicate_candidate_ids() {
        let preview = OpenSshImportPreview {
            candidates: vec![
                OpenSshImportCandidate::new(
                    SshProfile::new("prod", "Production A", "a.example.com"),
                    OpenSshImportSource::new("prod-a"),
                ),
                OpenSshImportCandidate::new(
                    SshProfile::new("prod", "Production B", "b.example.com"),
                    OpenSshImportSource::new("prod-b"),
                ),
            ],
            warnings: Vec::new(),
        };

        assert!(apply_openssh_import_preview(
            &mut ProfileStoreV1::new(),
            &preview,
            &OpenSshImportSelection::all(),
            OpenSshImportConflictPolicy::Replace,
        )
        .is_err());
        assert!(apply_openssh_import_preview(
            &mut ProfileStoreV1::new(),
            &preview,
            &OpenSshImportSelection::profile_ids(["prod"]),
            OpenSshImportConflictPolicy::Replace,
        )
        .is_err());
    }

    #[test]
    fn openssh_import_preview_warning_variants_cover_planned_parse_boundaries() {
        let warnings = vec![
            OpenSshImportWarning::UnsupportedDirective {
                keyword: "CanonicalizeHostname".to_owned(),
            },
            OpenSshImportWarning::UnsupportedPattern {
                pattern: "!prod".to_owned(),
            },
            OpenSshImportWarning::InvalidDirectiveValue {
                keyword: "Port".to_owned(),
                value: "not-a-port".to_owned(),
            },
            OpenSshImportWarning::MissingHostName {
                host_pattern: "prod".to_owned(),
            },
            OpenSshImportWarning::SkippedWildcardOnlyHost {
                pattern: "*".to_owned(),
            },
            OpenSshImportWarning::EmptyProfileId {
                host_pattern: "%%%".to_owned(),
            },
            OpenSshImportWarning::TokenExpansionPreserved {
                field: "IdentityFile".to_owned(),
            },
            OpenSshImportWarning::RemoteCommandImported,
            OpenSshImportWarning::ProxyCommandNotImported,
        ];

        let preview = OpenSshImportPreview {
            candidates: vec![OpenSshImportCandidate {
                profile: SshProfile::new("prod", "Production", "prod.example.com"),
                source: OpenSshImportSource::new("prod"),
                warnings,
                conflict: None,
            }],
            warnings: Vec::new(),
        };

        assert_eq!(preview.total_warning_count(), 9);
    }

    #[test]
    fn openssh_import_parser_imports_conservative_host_block_subset() {
        let config = r#"
            Host Prod.DB
                HostName prod.internal
                User deploy
                Port 2222
                IdentityFile ~/.ssh/prod_ed25519
                ProxyJump bastion
                RequestTTY force
                SetEnv TERM=xterm-witty
                RemoteCommand tmux new -A -s main
        "#;

        let preview =
            parse_openssh_import_preview(config, Some(PathBuf::from("/home/alice/.ssh/config")));

        assert!(preview.warnings.is_empty());
        assert_eq!(preview.candidates.len(), 1);
        let candidate = &preview.candidates[0];
        assert_eq!(candidate.profile.id, "prod.db");
        assert_eq!(candidate.profile.name, "Prod.DB");
        assert_eq!(
            candidate.profile.description.as_deref(),
            Some("Imported from OpenSSH config")
        );
        assert_eq!(candidate.profile.tags, vec!["imported", "openssh"]);
        assert_eq!(candidate.profile.target.host, "prod.internal");
        assert_eq!(candidate.profile.target.user.as_deref(), Some("deploy"));
        assert_eq!(candidate.profile.target.port, Some(2222));
        assert_eq!(
            candidate.profile.target.jump_host.as_deref(),
            Some("bastion")
        );
        assert_eq!(
            candidate.profile.terminal.term.as_deref(),
            Some("xterm-witty")
        );
        assert!(candidate.profile.terminal.request_tty);
        assert_eq!(
            candidate.profile.credential,
            SshCredentialRef::IdentityFile {
                path: PathBuf::from("~/.ssh/prod_ed25519"),
            }
        );
        assert_eq!(
            candidate.profile.openssh.remote_command,
            vec!["tmux", "new", "-A", "-s", "main"]
        );
        assert_eq!(candidate.source.host_pattern, "Prod.DB");
        assert_eq!(candidate.source.line, Some(2));
        assert_eq!(
            candidate.source.config_path,
            Some(PathBuf::from("/home/alice/.ssh/config"))
        );
        assert!(candidate
            .warnings
            .contains(&OpenSshImportWarning::IdentityFilePathOnly));
        assert!(candidate
            .warnings
            .contains(&OpenSshImportWarning::RemoteCommandImported));
        assert!(candidate
            .warnings
            .contains(&OpenSshImportWarning::TokenExpansionPreserved {
                field: "IdentityFile".to_owned()
            }));
    }

    #[test]
    fn openssh_import_parser_skips_wildcard_and_negated_patterns() {
        let config = r#"
            Host * !blocked concrete
                User deploy
        "#;

        let preview = parse_openssh_import_preview(config, None);

        assert_eq!(preview.candidates.len(), 1);
        assert_eq!(preview.candidates[0].profile.id, "concrete");
        assert_eq!(preview.candidates[0].profile.target.host, "concrete");
        assert!(preview.candidates[0]
            .warnings
            .contains(&OpenSshImportWarning::MissingHostName {
                host_pattern: "concrete".to_owned()
            }));
        assert!(preview
            .warnings
            .contains(&OpenSshImportWarning::SkippedWildcardOnlyHost {
                pattern: "*".to_owned()
            }));
        assert!(preview
            .warnings
            .contains(&OpenSshImportWarning::UnsupportedPattern {
                pattern: "!blocked".to_owned()
            }));
    }

    #[test]
    fn openssh_import_parser_warns_without_expanding_includes_or_proxy_commands() {
        let config = r#"
            Include conf.d/*.conf
            Host prod
                HostName prod.example.com
                Port not-a-port
                ProxyCommand nc %h %p
                CanonicalizeHostname yes
                SetEnv LANG=C
        "#;

        let preview = parse_openssh_import_preview(config, None);

        assert_eq!(preview.candidates.len(), 1);
        assert!(preview
            .warnings
            .contains(&OpenSshImportWarning::IncludeNotExpanded {
                path: "conf.d/*.conf".to_owned()
            }));
        let candidate = &preview.candidates[0];
        assert!(candidate
            .warnings
            .contains(&OpenSshImportWarning::ProxyCommandNotImported));
        assert!(candidate
            .warnings
            .contains(&OpenSshImportWarning::UnsupportedDirective {
                keyword: "CanonicalizeHostname".to_owned()
            }));
        assert!(candidate
            .warnings
            .contains(&OpenSshImportWarning::UnsupportedDirective {
                keyword: "SetEnv".to_owned()
            }));
        assert!(candidate
            .warnings
            .contains(&OpenSshImportWarning::InvalidDirectiveValue {
                keyword: "Port".to_owned(),
                value: "not-a-port".to_owned()
            }));
    }

    #[test]
    fn profile_store_v1_validates_launchable_and_resolver_required_profiles() {
        let mut agent_profile = SshProfile::new("prod", "Production", "prod.example.com");
        agent_profile.target.user("alice");
        let mut vault_profile = SshProfile::new("vaulted", "Vaulted", "vault.example.com");
        vault_profile.credential = SshCredentialRef::VaultSecret {
            secret_id: "vault-prod-key".to_owned(),
        };
        let mut store = ProfileStoreV1::with_profiles(vec![agent_profile, vault_profile]);
        store.default_profile_id = Some("prod".to_owned());

        let validation = store.validate().unwrap();
        let json = store.to_pretty_json().unwrap();
        let parsed = ProfileStoreV1::from_json(&json).unwrap();

        assert_eq!(
            validation,
            ProfileStoreValidation {
                launchable_profiles: 1,
                credential_resolver_required_profiles: 1,
            }
        );
        assert_eq!(
            parsed.profile("prod").unwrap().target.user.as_deref(),
            Some("alice")
        );
        assert_eq!(
            parsed.profile("vaulted").unwrap().launchability().unwrap(),
            SshProfileLaunchability::RequiresCredentialResolver
        );
        assert!(json.contains(PROFILE_STORE_APP));
        assert!(!json.contains("private_key"));
        assert!(!json.contains("passphrase"));
    }

    #[test]
    fn profile_store_redacted_summary_includes_only_ui_safe_fields() {
        let mut prod = SshProfile::new("prod", "Production", "prod.example.com");
        prod.tag("work");
        prod.target
            .user("alice")
            .port(2222)
            .jump_host("bastion.internal");
        prod.credential = SshCredentialRef::IdentityFile {
            path: PathBuf::from("/home/alice/.ssh/prod_ed25519"),
        };
        prod.openssh.config_file = Some(PathBuf::from("/home/alice/.ssh/config"));
        prod.openssh.extra_args = vec![
            "-o".to_owned(),
            "ServerAliveInterval=30".to_owned(),
            "-vv".to_owned(),
        ];
        prod.openssh.remote_command = vec!["uptime".to_owned()];

        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vault.example.com");
        vaulted.credential = SshCredentialRef::VaultSecret {
            secret_id: "vault-secret-prod".to_owned(),
        };

        let mut store = ProfileStoreV1::with_profiles(vec![prod, vaulted]);
        store.default_profile_id = Some("prod".to_owned());

        let summary = store.redacted_summary().unwrap();
        let json = serde_json::to_string(&summary).unwrap();

        assert_eq!(summary.default_profile_id.as_deref(), Some("prod"));
        assert_eq!(summary.profiles.len(), 2);
        assert_eq!(summary.profiles[0].id, "prod");
        assert_eq!(summary.profiles[0].name, "Production");
        assert_eq!(summary.profiles[0].tags, vec!["work"]);
        assert_eq!(
            summary.profiles[0].launchability,
            SshProfileLaunchability::Launchable
        );
        assert!(summary.profiles[0].is_default);
        assert!(json.contains("\"prod\""));
        assert!(json.contains("\"Production\""));
        assert!(json.contains("\"work\""));
        assert!(json.contains("\"launchable\""));
        assert!(json.contains("\"requires_credential_resolver\""));

        for forbidden in [
            "prod.example.com",
            "vault.example.com",
            "alice",
            "2222",
            "bastion.internal",
            "/home/alice/.ssh/prod_ed25519",
            "/home/alice/.ssh/config",
            "ServerAliveInterval",
            "-vv",
            "uptime",
            "vault-secret-prod",
            "\"target\"",
            "\"credential\"",
            "\"openssh\"",
            "\"remote_command\"",
            "\"secret_id\"",
        ] {
            assert!(
                !json.contains(forbidden),
                "redacted summary leaked {forbidden}: {json}"
            );
        }
    }

    #[test]
    fn profile_store_redacted_summary_rejects_unknown_fields() {
        let top_level = r#"{
            "profiles":[],
            "default_profile_id":null,
            "launchable_profiles":0,
            "credential_resolver_required_profiles":0,
            "store_path":"/home/user/.config/witty/profiles.v1.json"
        }"#;
        assert!(serde_json::from_str::<ProfileStoreSummary>(top_level).is_err());

        let nested_profile = r#"{
            "profiles":[{
                "id":"prod",
                "name":"Production",
                "tags":["work"],
                "launchability":"launchable",
                "is_default":true,
                "host":"prod.internal"
            }],
            "default_profile_id":"prod",
            "launchable_profiles":1,
            "credential_resolver_required_profiles":0
        }"#;
        assert!(serde_json::from_str::<ProfileStoreSummary>(nested_profile).is_err());
    }

    #[test]
    fn profile_store_redacted_summary_counts_launchability_and_default() {
        let prod = SshProfile::new("prod", "Production", "prod.example.com");
        let mut vaulted = SshProfile::new("vaulted", "Vaulted", "vault.example.com");
        vaulted.credential = SshCredentialRef::VaultSecret {
            secret_id: "vault-prod-key".to_owned(),
        };
        let mut store = ProfileStoreV1::with_profiles(vec![prod, vaulted]);
        store.default_profile_id = Some("vaulted".to_owned());

        let summary = store.redacted_summary().unwrap();

        assert_eq!(summary.launchable_profiles, 1);
        assert_eq!(summary.credential_resolver_required_profiles, 1);
        assert_eq!(summary.default_profile_id.as_deref(), Some("vaulted"));
        assert!(!summary.profiles[0].is_default);
        assert!(summary.profiles[1].is_default);

        store.default_profile_id = Some("missing".to_owned());
        assert!(store.redacted_summary().is_err());
    }

    #[test]
    fn profile_store_v1_rejects_unsupported_schema_and_app() {
        let mut store = ProfileStoreV1::new();
        store.schema = PROFILE_STORE_SCHEMA_V1 + 1;
        assert!(store.validate().is_err());

        let mut store = ProfileStoreV1::new();
        store.app = "other-app".to_owned();
        assert!(store.validate().is_err());
    }

    #[test]
    fn profile_store_v1_rejects_duplicate_ids_and_missing_default() {
        let duplicate_a = SshProfile::new("prod", "Production", "prod.example.com");
        let duplicate_b = SshProfile::new("prod", "Production Copy", "copy.example.com");
        let mut store = ProfileStoreV1::with_profiles(vec![duplicate_a, duplicate_b]);
        assert!(store.validate().is_err());

        store = ProfileStoreV1::with_profiles(vec![SshProfile::new(
            "prod",
            "Production",
            "prod.example.com",
        )]);
        store.default_profile_id = Some("missing".to_owned());
        assert!(store.validate().is_err());
    }

    #[test]
    fn profile_store_v1_enforces_profile_limits() {
        let mut profile = SshProfile::new("prod", "Production", "prod.example.com");
        profile.tags = (0..=PROFILE_STORE_MAX_TAGS_PER_PROFILE)
            .map(|index| format!("tag{index}"))
            .collect();
        let store = ProfileStoreV1::with_profiles(vec![profile]);
        assert!(store.validate().is_err());

        let mut profile = SshProfile::new("prod", "Production", "prod.example.com");
        profile.openssh.extra_args =
            vec!["-v".to_owned(); PROFILE_STORE_MAX_OPENSSH_EXTRA_ARGS + 1];
        let store = ProfileStoreV1::with_profiles(vec![profile]);
        assert!(store.validate().is_err());

        let mut profile = SshProfile::new("prod", "Production", "prod.example.com");
        profile.openssh.remote_command =
            vec!["echo".to_owned(); PROFILE_STORE_MAX_REMOTE_COMMAND_ARGS + 1];
        let store = ProfileStoreV1::with_profiles(vec![profile]);
        assert!(store.validate().is_err());
    }

    #[test]
    fn profile_store_v1_rejects_unknown_top_level_fields_and_oversized_json() {
        let json = r#"{"schema":1,"app":"witty-profiles","profiles":[],"default_profile_id":null,"future":true}"#;
        assert!(ProfileStoreV1::from_json(json).is_err());

        let oversized = " ".repeat(PROFILE_STORE_MAX_JSON_BYTES + 1);
        assert!(ProfileStoreV1::from_json(&oversized).is_err());
    }

    #[test]
    fn profile_store_add_profile_sets_default_when_empty() {
        let mut store = ProfileStoreV1::new();

        let mutation = store
            .add_profile(
                SshProfile::new("prod", "Production", "prod.example.com"),
                ProfileStoreDefaultPolicy::SetIfEmpty,
            )
            .unwrap();

        assert_eq!(
            mutation,
            ProfileStoreMutation {
                changed: true,
                profile_count: 1,
                default_profile_changed: true,
            }
        );
        assert_eq!(store.default_profile_id.as_deref(), Some("prod"));
    }

    #[test]
    fn profile_store_add_profile_rejects_duplicates_and_preserves_default() {
        let mut store = profile_store_with("prod", "prod.example.com");

        assert!(store
            .add_profile(
                SshProfile::new("prod", "Duplicate", "duplicate.example.com"),
                ProfileStoreDefaultPolicy::SetToAdded,
            )
            .is_err());

        let mutation = store
            .add_profile(
                SshProfile::new("staging", "Staging", "staging.example.com"),
                ProfileStoreDefaultPolicy::Preserve,
            )
            .unwrap();

        assert!(mutation.changed);
        assert_eq!(mutation.profile_count, 2);
        assert!(!mutation.default_profile_changed);
        assert_eq!(store.default_profile_id.as_deref(), Some("prod"));
    }

    #[test]
    fn profile_store_update_profile_rejects_rename_and_preserves_default() {
        let mut store = profile_store_with("prod", "prod.example.com");
        let mut updated = SshProfile::new("prod", "Production Updated", "new.example.com");
        updated.description("Updated profile");

        let mutation = store.update_profile("prod", updated).unwrap();

        assert!(mutation.changed);
        assert_eq!(mutation.profile_count, 1);
        assert!(!mutation.default_profile_changed);
        assert_eq!(
            store.profile("prod").unwrap().description.as_deref(),
            Some("Updated profile")
        );

        assert!(store
            .update_profile(
                "prod",
                SshProfile::new("renamed", "Renamed", "renamed.example.com"),
            )
            .is_err());
    }

    #[test]
    fn profile_store_remove_profile_clears_deleted_default() {
        let mut store = profile_store_with("prod", "prod.example.com");

        let mutation = store.remove_profile("prod").unwrap();

        assert_eq!(
            mutation,
            ProfileStoreMutation {
                changed: true,
                profile_count: 0,
                default_profile_changed: true,
            }
        );
        assert!(store.default_profile_id.is_none());
        assert!(store.profile("prod").is_none());
    }

    #[test]
    fn profile_store_set_default_profile_validates_existing_ids() {
        let mut store = profile_store_with("prod", "prod.example.com");
        store
            .add_profile(
                SshProfile::new("staging", "Staging", "staging.example.com"),
                ProfileStoreDefaultPolicy::Preserve,
            )
            .unwrap();

        let mutation = store.set_default_profile(Some("staging")).unwrap();
        assert!(mutation.default_profile_changed);
        assert_eq!(store.default_profile_id.as_deref(), Some("staging"));

        let mutation = store.set_default_profile(None).unwrap();
        assert!(mutation.default_profile_changed);
        assert!(store.default_profile_id.is_none());
        assert!(store.set_default_profile(Some("missing")).is_err());
    }

    #[test]
    fn profile_store_atomic_write_validates_before_creating_parent() {
        let root = unique_temp_dir("witty-profile-store-invalid-before-touch");
        let path = root.join("nested").join("profiles.v1.json");
        let mut store = profile_store_with("prod", "prod.example.com");
        store.app = "wrong-app".to_owned();

        assert!(write_profile_store_atomic(&path, &store).is_err());
        assert!(!root.exists());
    }

    #[test]
    fn profile_store_atomic_write_validates_before_creating_lock() {
        let root = unique_temp_dir("witty-profile-store-invalid-before-lock");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("profiles.v1.json");
        let mut store = profile_store_with("prod", "prod.example.com");
        store.default_profile_id = Some("missing".to_owned());

        assert!(write_profile_store_atomic(&path, &store).is_err());
        assert!(!path.exists());
        assert!(
            !profile_store_lock_path(path.parent().unwrap(), path.file_name().unwrap()).exists()
        );
        assert_eq!(profile_store_temp_file_count(&root), 0);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_atomic_write_creates_parent_and_loadable_json() {
        let root = unique_temp_dir("witty-profile-store-write");
        let path = root.join("nested").join("profiles.v1.json");
        let store = profile_store_with("prod", "prod.example.com");

        let report = write_profile_store_atomic(&path, &store).unwrap();
        let loaded = read_profile_store(&path).unwrap();

        assert_eq!(report.path, path);
        assert_eq!(report.bytes_written, fs::read(&path).unwrap().len());
        assert!(report.created_parent_dir);
        assert_eq!(
            loaded.profile("prod").unwrap().target.host,
            "prod.example.com"
        );
        assert_eq!(profile_store_temp_file_count(path.parent().unwrap()), 0);
        assert!(
            !profile_store_lock_path(path.parent().unwrap(), path.file_name().unwrap()).exists()
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_atomic_write_preserves_existing_file_on_validation_error() {
        let root = unique_temp_dir("witty-profile-store-preserve");
        let path = root.join("profiles.v1.json");
        let original = profile_store_with("prod", "prod.example.com");
        write_profile_store_atomic(&path, &original).unwrap();
        let original_bytes = fs::read(&path).unwrap();

        let mut invalid = profile_store_with("bad", "bad.example.com");
        invalid.default_profile_id = Some("missing".to_owned());

        assert!(write_profile_store_atomic(&path, &invalid).is_err());
        assert_eq!(fs::read(&path).unwrap(), original_bytes);
        assert_eq!(
            read_profile_store(&path)
                .unwrap()
                .profile("prod")
                .unwrap()
                .target
                .host,
            "prod.example.com"
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_atomic_write_replaces_existing_file() {
        let root = unique_temp_dir("witty-profile-store-replace");
        let path = root.join("profiles.v1.json");
        write_profile_store_atomic(&path, &profile_store_with("old", "old.example.com")).unwrap();

        let report =
            write_profile_store_atomic(&path, &profile_store_with("new", "new.example.com"))
                .unwrap();
        let loaded = read_profile_store(&path).unwrap();

        assert!(!report.created_parent_dir);
        assert!(loaded.profile("old").is_none());
        assert_eq!(
            loaded.profile("new").unwrap().target.host,
            "new.example.com"
        );
        assert_eq!(profile_store_temp_file_count(path.parent().unwrap()), 0);
        assert!(
            !profile_store_lock_path(path.parent().unwrap(), path.file_name().unwrap()).exists()
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_atomic_write_rejects_existing_lock_and_preserves_target() {
        let root = unique_temp_dir("witty-profile-store-existing-lock");
        let path = root.join("profiles.v1.json");
        write_profile_store_atomic(&path, &profile_store_with("old", "old.example.com")).unwrap();
        let original_bytes = fs::read(&path).unwrap();
        let lock_path = profile_store_lock_path(path.parent().unwrap(), path.file_name().unwrap());
        fs::write(&lock_path, "pid=external\n").unwrap();

        let error =
            write_profile_store_atomic(&path, &profile_store_with("new", "new.example.com"))
                .unwrap_err();

        assert!(error.to_string().contains("write lock"));
        assert_eq!(fs::read(&path).unwrap(), original_bytes);
        assert!(lock_path.exists());
        assert_eq!(profile_store_temp_file_count(path.parent().unwrap()), 0);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_write_lock_does_not_remove_replaced_lock_file() {
        let root = unique_temp_dir("witty-profile-store-replaced-lock");
        fs::create_dir_all(&root).unwrap();
        let file_name = OsStr::new("profiles.v1.json");
        let lock_path = profile_store_lock_path(&root, file_name);

        let lock = acquire_profile_store_write_lock(&root, file_name).unwrap();
        fs::write(&lock_path, "pid=external\n").unwrap();

        drop(lock);
        assert_eq!(fs::read_to_string(&lock_path).unwrap(), "pid=external\n");

        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn profile_store_write_lock_uses_restrictive_unix_permissions_and_removes_on_drop() {
        let root = unique_temp_dir("witty-profile-store-lock-permissions");
        fs::create_dir_all(&root).unwrap();
        let file_name = OsStr::new("profiles.v1.json");
        let lock_path = profile_store_lock_path(&root, file_name);

        let lock = acquire_profile_store_write_lock(&root, file_name).unwrap();

        let lock_mode = fs::metadata(&lock_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(lock_mode, 0o600);
        assert!(fs::read_to_string(&lock_path).unwrap().starts_with("pid="));

        drop(lock);
        assert!(!lock_path.exists());

        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn profile_store_atomic_write_uses_restrictive_unix_permissions() {
        let root = unique_temp_dir("witty-profile-store-permissions");
        let path = root.join("nested").join("profiles.v1.json");

        write_profile_store_atomic(&path, &profile_store_with("prod", "prod.example.com")).unwrap();

        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_edit_create_if_missing_adds_profile_under_lock() {
        let root = unique_temp_dir("witty-profile-store-edit-create");
        let path = root.join("nested").join("profiles.v1.json");

        let report =
            edit_profile_store(&path, ProfileStoreEditOpenMode::CreateIfMissing, |store| {
                store.add_profile(
                    SshProfile::new("prod", "Production", "prod.example.com"),
                    ProfileStoreDefaultPolicy::SetIfEmpty,
                )
            })
            .unwrap();
        let loaded = read_profile_store(&path).unwrap();

        assert!(report.write.created_parent_dir);
        assert!(report.mutation.changed);
        assert_eq!(report.mutation.profile_count, 1);
        assert_eq!(loaded.default_profile_id.as_deref(), Some("prod"));
        assert_eq!(
            loaded.profile("prod").unwrap().target.host,
            "prod.example.com"
        );
        assert!(
            !profile_store_lock_path(path.parent().unwrap(), path.file_name().unwrap()).exists()
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_edit_existing_missing_store_does_not_touch_filesystem() {
        let root = unique_temp_dir("witty-profile-store-edit-missing");
        let path = root.join("nested").join("profiles.v1.json");

        assert!(
            edit_profile_store(&path, ProfileStoreEditOpenMode::Existing, |store| {
                store.set_default_profile(None)
            })
            .is_err()
        );
        assert!(!root.exists());
    }

    #[test]
    fn profile_store_edit_existing_lock_preserves_target() {
        let root = unique_temp_dir("witty-profile-store-edit-locked");
        let path = root.join("profiles.v1.json");
        write_profile_store_atomic(&path, &profile_store_with("prod", "prod.example.com")).unwrap();
        let original_bytes = fs::read(&path).unwrap();
        let lock_path = profile_store_lock_path(path.parent().unwrap(), path.file_name().unwrap());
        fs::write(&lock_path, "pid=external\n").unwrap();

        let error = edit_profile_store(&path, ProfileStoreEditOpenMode::Existing, |store| {
            store.add_profile(
                SshProfile::new("staging", "Staging", "staging.example.com"),
                ProfileStoreDefaultPolicy::Preserve,
            )
        })
        .unwrap_err();

        assert!(error.to_string().contains("write lock"));
        assert_eq!(fs::read(&path).unwrap(), original_bytes);
        assert!(lock_path.exists());
        assert_eq!(profile_store_temp_file_count(path.parent().unwrap()), 0);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_edit_mutation_failure_preserves_target_and_removes_lock() {
        let root = unique_temp_dir("witty-profile-store-edit-failure");
        let path = root.join("profiles.v1.json");
        write_profile_store_atomic(&path, &profile_store_with("prod", "prod.example.com")).unwrap();
        let original_bytes = fs::read(&path).unwrap();

        assert!(
            edit_profile_store(&path, ProfileStoreEditOpenMode::Existing, |store| {
                store.add_profile(
                    SshProfile::new("prod", "Duplicate", "duplicate.example.com"),
                    ProfileStoreDefaultPolicy::SetToAdded,
                )
            })
            .is_err()
        );

        assert_eq!(fs::read(&path).unwrap(), original_bytes);
        assert!(
            !profile_store_lock_path(path.parent().unwrap(), path.file_name().unwrap()).exists()
        );
        assert_eq!(profile_store_temp_file_count(path.parent().unwrap()), 0);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn profile_store_edit_invalid_existing_json_preserves_target_and_removes_lock() {
        let root = unique_temp_dir("witty-profile-store-edit-invalid-json");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("profiles.v1.json");
        fs::write(&path, "{not json").unwrap();

        assert!(
            edit_profile_store(&path, ProfileStoreEditOpenMode::Existing, |store| {
                store.set_default_profile(None)
            })
            .is_err()
        );

        assert_eq!(fs::read_to_string(&path).unwrap(), "{not json");
        assert!(
            !profile_store_lock_path(path.parent().unwrap(), path.file_name().unwrap()).exists()
        );
        assert_eq!(profile_store_temp_file_count(path.parent().unwrap()), 0);

        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn default_profile_store_path_uses_xdg_config_home_on_linux_like_unix() {
        let path = default_profile_store_path_from_env(|name| match name {
            "XDG_CONFIG_HOME" => Some(OsString::from("/tmp/witty-config")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            path,
            PathBuf::from("/tmp/witty-config")
                .join("witty")
                .join("profiles.v1.json")
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn default_profile_store_path_falls_back_to_home_config_on_linux_like_unix() {
        let path = default_profile_store_path_from_env(|name| match name {
            "XDG_CONFIG_HOME" => Some(OsString::from("relative-config")),
            "HOME" => Some(OsString::from("/home/alice")),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            path,
            PathBuf::from("/home/alice")
                .join(".config")
                .join("witty")
                .join("profiles.v1.json")
        );
    }

    fn profile_store_with(id: &str, host: &str) -> ProfileStoreV1 {
        let mut profile = SshProfile::new(id, format!("Profile {id}"), host);
        profile.target.user("alice");
        let mut store = ProfileStoreV1::with_profiles(vec![profile]);
        store.default_profile_id = Some(id.to_owned());
        store
    }

    fn profile_store_temp_file_count(parent: &std::path::Path) -> usize {
        fs::read_dir(parent)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp."))
            .count()
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }
}

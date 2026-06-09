use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{self, Command},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use witty_core::GridSize;
use witty_transport::LocalPtyConfig;

pub const WITTY_APP_ID: &str = "dev.witty.Witty";
pub const INSTALL_STATE_FILE_NAME: &str = "install-state.v1.json";
pub const RESTART_STATE_FILE_PREFIX: &str = "restart-state.v1";
pub const INSTALL_STATE_SCHEMA_VERSION: u16 = 1;
pub const RESTART_STATE_SCHEMA_VERSION: u16 = 1;

const INSTALL_STATE_MAX_JSON_BYTES: u64 = 64 * 1024;
const RESTART_STATE_MAX_JSON_BYTES: u64 = 256 * 1024;
const WITTY_STATE_DIR_NAME: &str = "witty";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstalledBuildMarkerV1 {
    pub schema_version: u16,
    pub app_id: String,
    pub build_id: String,
    pub package_version: String,
    pub installed_at_utc: String,
    pub binary_path: PathBuf,
    pub install_prefix: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_profile: Option<String>,
}

impl InstalledBuildMarkerV1 {
    #[allow(dead_code)]
    pub fn new(
        build_id: impl Into<String>,
        package_version: impl Into<String>,
        installed_at_utc: impl Into<String>,
        binary_path: impl Into<PathBuf>,
        install_prefix: impl Into<PathBuf>,
        source_profile: Option<String>,
    ) -> Self {
        Self {
            schema_version: INSTALL_STATE_SCHEMA_VERSION,
            app_id: WITTY_APP_ID.to_owned(),
            build_id: build_id.into(),
            package_version: package_version.into(),
            installed_at_utc: installed_at_utc.into(),
            binary_path: binary_path.into(),
            install_prefix: install_prefix.into(),
            source_profile,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunningBuildIdentity {
    pub build_id: String,
    pub package_version: String,
    pub binary_path: PathBuf,
    pub installed_binary_path: Option<PathBuf>,
    pub source: RunningBuildSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunningBuildSource {
    InstalledMarker,
    RuntimeFallback,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledUpdateStatus {
    pub update_needed: bool,
    pub running_build_id: String,
    pub installed_build_id: Option<String>,
    pub installed_marker: Option<InstalledBuildMarkerV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestartSnapshotV1 {
    pub schema_version: u16,
    pub created_at_unix_ms: u128,
    pub running_build_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_build_id: Option<String>,
    pub window: RestartWindowStateV1,
    pub active_tab_index: usize,
    pub tabs: Vec<RestartTabV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestartWindowStateV1 {
    pub grid_rows: u16,
    pub grid_cols: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_width_px: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_height_px: Option<u32>,
}

impl RestartWindowStateV1 {
    pub fn grid_size(&self) -> GridSize {
        GridSize::new(self.grid_rows, self.grid_cols)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestartTabV1 {
    pub tab_id: u64,
    pub active: bool,
    pub source_plugin: String,
    pub profile_id: String,
    pub kind: RestartTabKindV1,
    pub mode: RestartTabModeV1,
    pub launch: RestartLaunchConfigV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<RestartProfileMetadataV1>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartTabKindV1 {
    Local,
    ProfilePicker,
    ProfileLaunch,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartTabModeV1 {
    ReplaceCurrentSession,
    NewTab,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestartProfileMetadataV1 {
    pub source_plugin: String,
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestartLaunchConfigV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: Vec<RestartEnvVarV1>,
}

impl RestartLaunchConfigV1 {
    pub fn from_local_pty_config(config: &LocalPtyConfig) -> Self {
        Self {
            program: config.program.clone(),
            args: config.args.clone(),
            cwd: config.cwd.clone(),
            env: restart_env_metadata(&config.env),
        }
    }

    pub fn to_local_pty_config(&self, size: GridSize) -> LocalPtyConfig {
        LocalPtyConfig {
            size,
            program: self.program.clone(),
            args: self.args.clone(),
            env: self
                .env
                .iter()
                .filter_map(|entry| entry.restored_value())
                .collect(),
            cwd: self.cwd.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestartEnvVarV1 {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub restored: bool,
    pub redacted: bool,
}

impl RestartEnvVarV1 {
    fn restored_value(&self) -> Option<(String, String)> {
        if !self.restored || self.redacted {
            return None;
        }
        Some((self.key.clone(), self.value.clone()?))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestartExecutionPlan {
    pub binary_path: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub snapshot_path: PathBuf,
}

pub fn default_install_state_path() -> Result<PathBuf> {
    default_install_state_path_with_env(|name| env::var_os(name))
}

pub fn default_install_state_path_with_env(
    env_var: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    Ok(default_state_root_with_env(env_var)?
        .join(WITTY_STATE_DIR_NAME)
        .join(INSTALL_STATE_FILE_NAME))
}

pub fn default_restart_state_path() -> Result<PathBuf> {
    default_restart_state_path_with_env(|name| env::var_os(name), process::id())
}

pub fn default_restart_state_path_with_env(
    env_var: impl Fn(&str) -> Option<OsString>,
    pid: u32,
) -> Result<PathBuf> {
    Ok(default_state_root_with_env(env_var)?
        .join(WITTY_STATE_DIR_NAME)
        .join(format!("{RESTART_STATE_FILE_PREFIX}.{pid}.json")))
}

pub fn read_installed_build_marker(path: &Path) -> Result<Option<InstalledBuildMarkerV1>> {
    let marker = read_json_file(path, INSTALL_STATE_MAX_JSON_BYTES)?;
    if let Some(marker) = &marker {
        validate_installed_build_marker(marker)
            .with_context(|| format!("validate install marker {}", path.display()))?;
    }
    Ok(marker)
}

#[allow(dead_code)]
pub fn write_installed_build_marker_atomic(
    path: &Path,
    marker: &InstalledBuildMarkerV1,
) -> Result<()> {
    validate_installed_build_marker(marker)?;
    write_json_atomic(path, marker)
}

pub fn read_restart_snapshot(path: &Path) -> Result<Option<RestartSnapshotV1>> {
    let snapshot = read_json_file(path, RESTART_STATE_MAX_JSON_BYTES)?;
    if let Some(snapshot) = &snapshot {
        validate_restart_snapshot(snapshot)
            .with_context(|| format!("validate restart snapshot {}", path.display()))?;
    }
    Ok(snapshot)
}

pub fn write_restart_snapshot_atomic(path: &Path, snapshot: &RestartSnapshotV1) -> Result<()> {
    validate_restart_snapshot(snapshot)?;
    write_json_atomic(path, snapshot)
}

pub fn running_build_identity(
    current_exe: PathBuf,
    installed_marker: Option<&InstalledBuildMarkerV1>,
    package_version: &str,
) -> RunningBuildIdentity {
    if let Some(marker) = installed_marker {
        if paths_equivalent(&current_exe, &marker.binary_path) {
            return RunningBuildIdentity {
                build_id: marker.build_id.clone(),
                package_version: marker.package_version.clone(),
                binary_path: current_exe,
                installed_binary_path: Some(marker.binary_path.clone()),
                source: RunningBuildSource::InstalledMarker,
            };
        }
    }

    RunningBuildIdentity {
        build_id: format!("witty-app:{package_version}:{}", current_exe.display()),
        package_version: package_version.to_owned(),
        binary_path: current_exe,
        installed_binary_path: None,
        source: RunningBuildSource::RuntimeFallback,
    }
}

pub fn installed_update_status(
    running: &RunningBuildIdentity,
    installed_marker: Option<InstalledBuildMarkerV1>,
) -> InstalledUpdateStatus {
    let update_needed = installed_marker.as_ref().is_some_and(|marker| {
        let running_installed_path = running
            .installed_binary_path
            .as_deref()
            .unwrap_or(&running.binary_path);
        paths_equivalent(running_installed_path, &marker.binary_path)
            && running.build_id != marker.build_id
    });

    InstalledUpdateStatus {
        update_needed,
        running_build_id: running.build_id.clone(),
        installed_build_id: installed_marker
            .as_ref()
            .map(|marker| marker.build_id.clone()),
        installed_marker,
    }
}

pub fn plan_restart_execution(
    installed_marker: &InstalledBuildMarkerV1,
    snapshot_path: impl Into<PathBuf>,
) -> RestartExecutionPlan {
    let snapshot_path = snapshot_path.into();
    RestartExecutionPlan {
        binary_path: installed_marker.binary_path.clone(),
        args: vec![
            "--window".to_owned(),
            "--restore-state".to_owned(),
            snapshot_path.display().to_string(),
        ],
        env: vec![("WGPU_BACKEND".to_owned(), "gl".to_owned())],
        snapshot_path,
    }
}

pub fn spawn_restart_plan(plan: &RestartExecutionPlan) -> Result<()> {
    let mut command = Command::new(&plan.binary_path);
    command.args(&plan.args);
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    command
        .spawn()
        .with_context(|| format!("spawn restart binary {}", plan.binary_path.display()))?;
    Ok(())
}

pub fn restart_env_metadata(env: &[(String, String)]) -> Vec<RestartEnvVarV1> {
    env.iter()
        .map(|(key, value)| {
            let safe = is_safe_restart_env_key(key);
            RestartEnvVarV1 {
                key: key.clone(),
                value: safe.then(|| value.clone()),
                restored: safe,
                redacted: !safe,
            }
        })
        .collect()
}

pub fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn default_state_root_with_env(env_var: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    if let Some(path) = env_var("XDG_STATE_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    let home = env_var("HOME").context("HOME is not set; cannot resolve Witty state path")?;
    if home.is_empty() {
        bail!("HOME is empty; cannot resolve Witty state path");
    }
    Ok(PathBuf::from(home).join(".local").join("state"))
}

fn read_json_file<T>(path: &Path, max_json_bytes: u64) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("stat {}", path.display())),
    };
    if !metadata.is_file() {
        bail!("state path is not a file: {}", path.display());
    }
    if metadata.len() > max_json_bytes {
        bail!(
            "state file exceeds {} bytes: {}",
            max_json_bytes,
            path.display()
        );
    }

    let json = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
}

fn write_json_atomic<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let parent = path
        .parent()
        .with_context(|| format!("state path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;

    let json = {
        let mut json = serde_json::to_string_pretty(value).context("serialize state JSON")?;
        json.push('\n');
        json
    };

    let mut last_error = None;
    for attempt in 0..32 {
        let temp_path = atomic_temp_path(path, attempt);
        let write_result = (|| -> Result<()> {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            let mut file = options
                .open(&temp_path)
                .with_context(|| format!("create temp state {}", temp_path.display()))?;
            use std::io::Write as _;
            file.write_all(json.as_bytes())
                .with_context(|| format!("write temp state {}", temp_path.display()))?;
            file.sync_all()
                .with_context(|| format!("sync temp state {}", temp_path.display()))?;
            fs::rename(&temp_path, path)
                .with_context(|| format!("rename {} to {}", temp_path.display(), path.display()))
        })();

        match write_result {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("failed to write state {}", path.display())))
}

fn atomic_temp_path(path: &Path, attempt: u32) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("state.json");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_file_name(format!(
        ".{file_name}.tmp.{}.{}.{}",
        process::id(),
        nonce,
        attempt
    ))
}

fn validate_installed_build_marker(marker: &InstalledBuildMarkerV1) -> Result<()> {
    if marker.schema_version != INSTALL_STATE_SCHEMA_VERSION {
        bail!(
            "unsupported install marker schema_version {}; expected {}",
            marker.schema_version,
            INSTALL_STATE_SCHEMA_VERSION
        );
    }
    if marker.app_id != WITTY_APP_ID {
        bail!("install marker app_id {:?} is not Witty", marker.app_id);
    }
    if marker.build_id.trim().is_empty() {
        bail!("install marker build_id cannot be empty");
    }
    if marker.package_version.trim().is_empty() {
        bail!("install marker package_version cannot be empty");
    }
    if marker.installed_at_utc.trim().is_empty() {
        bail!("install marker installed_at_utc cannot be empty");
    }
    if marker.binary_path.as_os_str().is_empty() {
        bail!("install marker binary_path cannot be empty");
    }
    if marker.install_prefix.as_os_str().is_empty() {
        bail!("install marker install_prefix cannot be empty");
    }
    Ok(())
}

fn validate_restart_snapshot(snapshot: &RestartSnapshotV1) -> Result<()> {
    if snapshot.schema_version != RESTART_STATE_SCHEMA_VERSION {
        bail!(
            "unsupported restart snapshot schema_version {}; expected {}",
            snapshot.schema_version,
            RESTART_STATE_SCHEMA_VERSION
        );
    }
    if snapshot.window.grid_rows == 0 || snapshot.window.grid_cols == 0 {
        bail!("restart snapshot window grid cannot be empty");
    }
    if snapshot.tabs.is_empty() {
        bail!("restart snapshot must contain at least one tab");
    }
    if snapshot.active_tab_index >= snapshot.tabs.len() {
        bail!(
            "restart snapshot active_tab_index {} is out of range for {} tabs",
            snapshot.active_tab_index,
            snapshot.tabs.len()
        );
    }
    let active_count = snapshot.tabs.iter().filter(|tab| tab.active).count();
    if active_count != 1 || !snapshot.tabs[snapshot.active_tab_index].active {
        bail!("restart snapshot must have exactly one active tab matching active_tab_index");
    }
    Ok(())
}

fn paths_equivalent(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn is_safe_restart_env_key(key: &str) -> bool {
    key == "TERM"
        || key == "COLORTERM"
        || key == "LANG"
        || key.starts_with("LC_")
        || key.starts_with("WITTY_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    fn marker(build_id: &str, binary_path: impl Into<PathBuf>) -> InstalledBuildMarkerV1 {
        InstalledBuildMarkerV1::new(
            build_id,
            "0.1.0",
            "2026-06-08T10:00:00Z",
            binary_path,
            "/home/test/.local",
            Some("debug".to_owned()),
        )
    }

    fn snapshot() -> RestartSnapshotV1 {
        RestartSnapshotV1 {
            schema_version: RESTART_STATE_SCHEMA_VERSION,
            created_at_unix_ms: 1,
            running_build_id: "old".to_owned(),
            installed_build_id: Some("new".to_owned()),
            window: RestartWindowStateV1 {
                grid_rows: 36,
                grid_cols: 120,
                inner_width_px: Some(1280),
                inner_height_px: Some(720),
            },
            active_tab_index: 1,
            tabs: vec![
                RestartTabV1 {
                    tab_id: 10,
                    active: false,
                    source_plugin: "witty-local".to_owned(),
                    profile_id: "local-1".to_owned(),
                    kind: RestartTabKindV1::Local,
                    mode: RestartTabModeV1::ReplaceCurrentSession,
                    launch: RestartLaunchConfigV1 {
                        program: None,
                        args: Vec::new(),
                        cwd: Some(PathBuf::from("/work")),
                        env: vec![RestartEnvVarV1 {
                            key: "TERM".to_owned(),
                            value: Some("xterm-256color".to_owned()),
                            restored: true,
                            redacted: false,
                        }],
                    },
                    profile: None,
                },
                RestartTabV1 {
                    tab_id: 11,
                    active: true,
                    source_plugin: "profiles".to_owned(),
                    profile_id: "prod".to_owned(),
                    kind: RestartTabKindV1::ProfileLaunch,
                    mode: RestartTabModeV1::NewTab,
                    launch: RestartLaunchConfigV1 {
                        program: Some("ssh".to_owned()),
                        args: vec!["prod.example.com".to_owned()],
                        cwd: None,
                        env: Vec::new(),
                    },
                    profile: Some(RestartProfileMetadataV1 {
                        source_plugin: "profiles".to_owned(),
                        profile_id: "prod".to_owned(),
                        reason: Some("open production".to_owned()),
                    }),
                },
            ],
        }
    }

    #[test]
    fn default_state_paths_use_xdg_state_home_or_fake_home() {
        let xdg = default_install_state_path_with_env(|name| match name {
            "XDG_STATE_HOME" => Some(OsString::from("/tmp/xdg-state")),
            "HOME" => Some(OsString::from("/tmp/home")),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            xdg,
            PathBuf::from("/tmp/xdg-state/witty/install-state.v1.json")
        );

        let home = default_restart_state_path_with_env(
            |name| match name {
                "HOME" => Some(OsString::from("/tmp/home")),
                _ => None,
            },
            42,
        )
        .unwrap();
        assert_eq!(
            home,
            PathBuf::from("/tmp/home/.local/state/witty/restart-state.v1.42.json")
        );
    }

    #[test]
    fn install_marker_atomic_round_trip_and_missing_file() {
        let root = unique_temp_dir("witty-install-marker");
        let path = root.join("state").join(INSTALL_STATE_FILE_NAME);
        assert!(read_installed_build_marker(&path).unwrap().is_none());

        let expected = marker("build-a", "/tmp/witty");
        write_installed_build_marker_atomic(&path, &expected).unwrap();
        let loaded = read_installed_build_marker(&path).unwrap().unwrap();
        assert_eq!(loaded, expected);
    }

    #[test]
    fn install_marker_rejects_unknown_fields_and_bad_schema() {
        let unknown = serde_json::json!({
            "schema_version": 1,
            "app_id": WITTY_APP_ID,
            "build_id": "build-a",
            "package_version": "0.1.0",
            "installed_at_utc": "2026-06-08T10:00:00Z",
            "binary_path": "/tmp/witty",
            "install_prefix": "/tmp/prefix",
            "extra": true
        });
        assert!(serde_json::from_value::<InstalledBuildMarkerV1>(unknown).is_err());

        let mut bad = marker("build-a", "/tmp/witty");
        bad.schema_version = 999;
        assert!(write_installed_build_marker_atomic(
            &unique_temp_dir("witty-install-bad").join("install-state.v1.json"),
            &bad,
        )
        .is_err());
    }

    #[test]
    fn running_installed_build_detects_new_marker_mismatch() {
        let current = marker("old", "/opt/witty/bin/witty");
        let running = running_build_identity(
            PathBuf::from("/opt/witty/bin/witty"),
            Some(&current),
            "0.1.0",
        );
        assert_eq!(running.source, RunningBuildSource::InstalledMarker);

        let same = installed_update_status(&running, Some(current.clone()));
        assert!(!same.update_needed);

        let updated = marker("new", "/opt/witty/bin/witty");
        let status = installed_update_status(&running, Some(updated.clone()));
        assert!(status.update_needed);
        assert_eq!(status.running_build_id, "old");
        assert_eq!(status.installed_build_id.as_deref(), Some("new"));
        assert_eq!(status.installed_marker, Some(updated));
    }

    #[test]
    fn non_installed_runtime_does_not_report_installed_marker_update() {
        let installed = marker("new", "/home/test/.local/bin/witty");
        let running = running_build_identity(
            PathBuf::from("/home/test/src/witty/target/debug/witty"),
            Some(&installed),
            "0.1.0",
        );
        assert_eq!(running.source, RunningBuildSource::RuntimeFallback);
        assert!(!installed_update_status(&running, Some(installed)).update_needed);
    }

    #[test]
    fn installed_runtime_without_startup_marker_detects_later_marker() {
        let running =
            running_build_identity(PathBuf::from("/home/test/.local/bin/witty"), None, "0.1.0");
        assert_eq!(running.source, RunningBuildSource::RuntimeFallback);

        let installed = marker("new", "/home/test/.local/bin/witty");
        let status = installed_update_status(&running, Some(installed));
        assert!(status.update_needed);
        assert!(status.running_build_id.starts_with("witty-app:0.1.0:"));
        assert_eq!(status.installed_build_id.as_deref(), Some("new"));
    }

    #[test]
    fn restart_env_metadata_restores_only_safe_keys() {
        let env = restart_env_metadata(&[
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("WITTY_SESSION".to_owned(), "daily".to_owned()),
            ("SECRET_TOKEN".to_owned(), "not-stored".to_owned()),
        ]);

        assert_eq!(
            env,
            vec![
                RestartEnvVarV1 {
                    key: "TERM".to_owned(),
                    value: Some("xterm-256color".to_owned()),
                    restored: true,
                    redacted: false,
                },
                RestartEnvVarV1 {
                    key: "WITTY_SESSION".to_owned(),
                    value: Some("daily".to_owned()),
                    restored: true,
                    redacted: false,
                },
                RestartEnvVarV1 {
                    key: "SECRET_TOKEN".to_owned(),
                    value: None,
                    restored: false,
                    redacted: true,
                },
            ]
        );
    }

    #[test]
    fn restart_launch_config_round_trips_restorable_env_to_local_pty_config() {
        let mut config = LocalPtyConfig::new(GridSize::new(24, 80));
        config.program = Some("/bin/zsh".to_owned());
        config.args(["-l"]);
        config.cwd("/work/project");
        config.env("SECRET_TOKEN", "not-stored");

        let launch = RestartLaunchConfigV1::from_local_pty_config(&config);
        assert!(launch
            .env
            .iter()
            .any(|entry| entry.key == "SECRET_TOKEN" && entry.redacted));

        let restored = launch.to_local_pty_config(GridSize::new(36, 120));
        assert_eq!(restored.size, GridSize::new(36, 120));
        assert_eq!(restored.program.as_deref(), Some("/bin/zsh"));
        assert_eq!(restored.args, vec!["-l"]);
        assert_eq!(restored.cwd, Some(PathBuf::from("/work/project")));
        assert!(restored.env.iter().any(|(key, _)| key == "TERM"));
        assert!(restored.env.iter().any(|(key, _)| key == "COLORTERM"));
        assert!(!restored.env.iter().any(|(key, _)| key == "SECRET_TOKEN"));
    }

    #[test]
    fn restart_snapshot_atomic_round_trip_and_validation() {
        let root = unique_temp_dir("witty-restart-snapshot");
        let path = root.join("state").join("restart-state.v1.1.json");
        let expected = snapshot();

        write_restart_snapshot_atomic(&path, &expected).unwrap();
        let loaded = read_restart_snapshot(&path).unwrap().unwrap();
        assert_eq!(loaded, expected);

        let mut invalid = snapshot();
        invalid.active_tab_index = 0;
        assert!(write_restart_snapshot_atomic(&path, &invalid).is_err());
    }

    #[test]
    fn restart_snapshot_rejects_unknown_fields() {
        let mut value = serde_json::to_value(snapshot()).unwrap();
        value.as_object_mut().unwrap().insert(
            "terminal_text".to_owned(),
            serde_json::Value::String("leak".to_owned()),
        );
        assert!(serde_json::from_value::<RestartSnapshotV1>(value).is_err());
    }

    #[test]
    fn restart_execution_plan_targets_installed_binary_without_spawning() {
        let marker = marker("new", "/home/test/.local/bin/witty");
        let plan = plan_restart_execution(&marker, "/tmp/restart-state.v1.123.json");

        assert_eq!(
            plan.binary_path,
            PathBuf::from("/home/test/.local/bin/witty")
        );
        assert_eq!(
            plan.args,
            vec![
                "--window".to_owned(),
                "--restore-state".to_owned(),
                "/tmp/restart-state.v1.123.json".to_owned(),
            ]
        );
        assert_eq!(plan.env, vec![("WGPU_BACKEND".to_owned(), "gl".to_owned())]);
    }
}

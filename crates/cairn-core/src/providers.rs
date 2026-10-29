//! LLM providers and their models (spec §6.1, §6.1a, UI-SET-2). Keys are kept in a
//! [`SecretStore`] (NFR-SEC-1), never in the database; `Provider::has_key` is read from it.
//!
//! The types are the contract with the UI and the
//! sidecar (`sidecar/PROTOCOL.md`, "The `target` object").

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::secrets::SecretStore;
use crate::util::{new_id, now_iso};
use crate::{CoreError, Db, Result};

/// Levels offered when a model's effort levels are not customised (FR-LLM-EF-4).
pub const DEFAULT_EFFORT_LEVELS: &[&str] = &["low", "medium", "high"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Azure AI Foundry (FR-LLM-PV-1, -1a, -7): `/openai/v1`, the older `api-version` style, or
    /// `/anthropic` for Claude deployments; detected from the endpoint URL.
    AzureFoundry,
    /// Claude API (FR-LLM-PV-2).
    Anthropic,
    /// Any OpenAI-format endpoint (FR-LLM-PV-3).
    OpenaiCompatible,
    /// Ollama / LM Studio (FR-LLM-PV-4): OpenAI-compatible, usually without a key.
    Local,
    /// DeepSeek's own API (D-69): Chat Completions with an API key; an empty endpoint is
    /// `https://api.deepseek.com`. Its web search is run by Cairn through DeepSeek's Search and
    /// Fetch API, as tools the model calls.
    #[serde(rename = "deepseek")]
    DeepSeek,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    ApiKey,
    /// Microsoft Entra ID through the user's `az login` (FR-LLM-PV-1a).
    AzureCli,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RequestFormat {
    ChatCompletions,
    Responses,
    AnthropicMessages,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    /// Base URL; empty means the provider's default (Claude API) or `http://localhost:11434/v1` (local).
    pub endpoint: String,
    /// Only for the older Azure endpoint style.
    pub api_version: Option<String>,
    pub auth: AuthMode,
    /// FR-LLM-PV-6: a disabled provider keeps its settings and key, but its models are hidden.
    pub enabled: bool,
    /// Whether a key is saved in the secure store (always false unless `auth` is `api_key`).
    pub has_key: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewProvider {
    pub name: String,
    pub kind: ProviderKind,
    #[specta(optional)]
    pub endpoint: Option<String>,
    #[specta(optional)]
    pub api_version: Option<String>,
    pub auth: AuthMode,
}

/// Only the given fields change. `apiVersion: ""` clears it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPatch {
    #[specta(optional)]
    pub name: Option<String>,
    #[specta(optional)]
    pub endpoint: Option<String>,
    #[specta(optional)]
    pub api_version: Option<String>,
    #[specta(optional)]
    pub auth: Option<AuthMode>,
    #[specta(optional)]
    pub enabled: Option<bool>,
}

/// FR-LLM-EF-1..4. Every field is optional in effect: `None` uses the built-in default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EffortConfig {
    /// FR-LLM-EF-3: effort is never sent to a model unless enabled.
    pub enabled: bool,
    /// Levels the chat picker offers; `None` = [`DEFAULT_EFFORT_LEVELS`].
    pub levels: Option<Vec<String>>,
    /// Used when the chat doesn't choose a level; `None` = nothing is sent (FR-LLM-EF-1).
    pub default_level: Option<String>,
    /// "How it is sent": `None` = Auto (FR-LLM-EF-2); otherwise a value per level, e.g. a Claude
    /// thinking budget in tokens ("4096").
    pub custom: Option<BTreeMap<String, String>>,
}

impl EffortConfig {
    /// The level a call sends (D-65), as the AI worker decides it (`resolve_level` in
    /// sidecar/cairn_sidecar/ai.py): none unless enabled (FR-LLM-EF-3), else the picked level,
    /// else the default level, else none (FR-LLM-EF-1).
    pub fn level_to_send(&self, picked: Option<&str>) -> Option<String> {
        if !self.enabled {
            return None;
        }
        picked
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .or_else(|| self.default_level.clone().filter(|d| !d.is_empty()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    pub id: String,
    pub provider_id: String,
    /// Deployment / model name sent to the API.
    pub name: String,
    /// Shown in the pickers.
    pub label: String,
    pub format: RequestFormat,
    pub vision: bool,
    pub effort: EffortConfig,
    /// FR-KN-UP-4a: how many tokens the model accepts; `None` = LiteLLM's known size, or try.
    pub context_size: Option<u32>,
    /// FR-LLM-AS-12: the provider offers built-in web search, so the composer shows 🌐. Removed
    /// automatically when the provider refuses or ignores it ([`set_web_search`]).
    pub web_search: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewModel {
    pub provider_id: String,
    pub name: String,
    /// Default: the name.
    #[specta(optional)]
    pub label: Option<String>,
    pub format: RequestFormat,
    #[specta(optional)]
    pub vision: Option<bool>,
    #[specta(optional)]
    pub effort: Option<EffortConfig>,
    #[specta(optional)]
    pub context_size: Option<u32>,
    #[specta(optional)]
    pub web_search: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelPatch {
    #[specta(optional)]
    pub name: Option<String>,
    #[specta(optional)]
    pub label: Option<String>,
    #[specta(optional)]
    pub format: Option<RequestFormat>,
    #[specta(optional)]
    pub vision: Option<bool>,
    #[specta(optional)]
    pub effort: Option<EffortConfig>,
    /// `0` clears it.
    #[specta(optional)]
    pub context_size: Option<u32>,
    #[specta(optional)]
    pub web_search: Option<bool>,
}

/// Effort as the sidecar receives it: levels always filled in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetEffort {
    pub enabled: bool,
    pub levels: Vec<String>,
    pub default_level: Option<String>,
    pub custom: Option<BTreeMap<String, String>>,
}

/// Everything the sidecar needs for one call (`sidecar/PROTOCOL.md`, "The `target` object").
/// Serialised with snake_case names on purpose; it never goes to the UI (it may hold a key).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct CallTarget {
    pub provider_kind: ProviderKind,
    pub endpoint: String,
    pub api_version: Option<String>,
    pub auth: AuthMode,
    /// Read from the secure store just before the call (NFR-SEC-1).
    pub api_key: Option<String>,
    /// Empty for provider-level calls (fetch models).
    pub model: String,
    pub format: RequestFormat,
    pub vision: bool,
    /// FR-LLM-AS-12: `llm.test` then also tries one web search.
    pub web_search: bool,
    pub effort: TargetEffort,
}

impl std::fmt::Debug for CallTarget {
    // Never print the key (NFR-SEC-3).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallTarget")
            .field("provider_kind", &self.provider_kind)
            .field("endpoint", &self.endpoint)
            .field("auth", &self.auth)
            .field("api_key", &self.api_key.as_ref().map(|_| "<set>"))
            .field("model", &self.model)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------- Azure endpoint styles

/// The three ways an Azure AI Foundry endpoint can look (FR-LLM-PV-1, -1a, -7). The style is
/// read from the URL, never chosen separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AzureStyle {
    /// `…/openai/v1`: OpenAI-compatible, no `api-version`.
    OpenAiV1,
    /// `…/anthropic`: Claude deployments (Messages API).
    Anthropic,
    /// Anything else: the older style that needs an `api-version`.
    Classic,
}

/// Detects the Azure style from the endpoint URL.
pub fn azure_style(endpoint: &str) -> AzureStyle {
    let url = endpoint.trim().to_ascii_lowercase();
    let path = url
        .split_once("://")
        .map_or(url.as_str(), |(_, rest)| rest)
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let path = path.trim_end_matches('/');
    if path.contains("/openai/v1") {
        AzureStyle::OpenAiV1
    } else if path.ends_with("/anthropic") || path.contains("/anthropic/") {
        AzureStyle::Anthropic
    } else {
        AzureStyle::Classic
    }
}

/// Endpoint used when a local provider's endpoint is left empty (FR-LLM-PV-4: Ollama).
pub const LOCAL_DEFAULT_ENDPOINT: &str = "http://localhost:11434/v1";

// ---------------------------------------------------------------- validation

fn invalid<T>(message: impl Into<String>) -> Result<T> {
    Err(CoreError::Invalid(message.into()))
}

fn clean_name(name: &str, what: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return invalid(format!("Give the {what} a name."));
    }
    Ok(name.to_string())
}

/// Allowed authentication per kind (FR-LLM-PV-1..4).
fn auth_allowed(kind: ProviderKind, auth: AuthMode) -> bool {
    match kind {
        ProviderKind::AzureFoundry => matches!(auth, AuthMode::ApiKey | AuthMode::AzureCli),
        ProviderKind::Anthropic => auth == AuthMode::ApiKey,
        ProviderKind::OpenaiCompatible => matches!(auth, AuthMode::ApiKey | AuthMode::None),
        ProviderKind::Local => matches!(auth, AuthMode::None | AuthMode::ApiKey),
        ProviderKind::DeepSeek => auth == AuthMode::ApiKey,
    }
}

/// Trims the endpoint, checks it and applies the per-kind default. Returns the stored form
/// (no trailing slash).
fn clean_endpoint(kind: ProviderKind, endpoint: &str) -> Result<String> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return match kind {
            ProviderKind::Anthropic | ProviderKind::DeepSeek => Ok(String::new()),
            ProviderKind::Local => Ok(LOCAL_DEFAULT_ENDPOINT.to_string()),
            ProviderKind::AzureFoundry | ProviderKind::OpenaiCompatible => {
                invalid("Enter the endpoint URL, for example https://example.com/v1.")
            }
        };
    }
    let rest = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"));
    match rest {
        Some(r) if !r.is_empty() && !r.contains(char::is_whitespace) => Ok(endpoint.to_string()),
        _ => invalid("The endpoint must be a web address starting with http:// or https://."),
    }
}

/// Everything about a provider that has to be consistent, checked on create and update.
/// Returns the cleaned `(endpoint, api_version)`.
fn check_provider_fields(
    kind: ProviderKind,
    endpoint: &str,
    api_version: Option<&str>,
    auth: AuthMode,
) -> Result<(String, Option<String>)> {
    if !auth_allowed(kind, auth) {
        return invalid(match kind {
            ProviderKind::AzureFoundry => {
                "Azure AI Foundry uses an API key or the Azure CLI sign-in."
            }
            ProviderKind::Anthropic => "The Claude API needs an API key.",
            ProviderKind::OpenaiCompatible => "This provider uses an API key or no authentication.",
            ProviderKind::Local => "A local provider uses no authentication or an API key.",
            ProviderKind::DeepSeek => "DeepSeek needs an API key.",
        });
    }
    let endpoint = clean_endpoint(kind, endpoint)?;
    let version = api_version.map(str::trim).filter(|v| !v.is_empty());
    let version = match (kind, version) {
        (_, None) => None,
        (ProviderKind::AzureFoundry, Some(v)) => {
            // Only the older style has an api-version; switching the URL drops it quietly.
            (azure_style(&endpoint) == AzureStyle::Classic).then(|| v.to_string())
        }
        (_, Some(_)) => return invalid("An API version only applies to Azure AI Foundry."),
    };
    Ok((endpoint, version))
}

fn clean_levels(levels: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for level in levels.iter().map(|l| l.trim()) {
        if level.is_empty() {
            return invalid("An effort level cannot be empty.");
        }
        if out.iter().any(|l| l.eq_ignore_ascii_case(level)) {
            return invalid(format!("The effort level \"{level}\" is listed twice."));
        }
        out.push(level.to_string());
    }
    if out.is_empty() {
        return invalid("Add at least one effort level, or turn effort off.");
    }
    Ok(out)
}

/// FR-LLM-EF-4: levels non-empty, default is one of the levels, custom keys are levels.
fn clean_effort(effort: EffortConfig) -> Result<EffortConfig> {
    let levels = effort.levels.as_deref().map(clean_levels).transpose()?;
    let effective: Vec<String> = levels.clone().unwrap_or_else(|| {
        DEFAULT_EFFORT_LEVELS
            .iter()
            .map(|l| l.to_string())
            .collect()
    });
    let default_level = effort
        .default_level
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    if let Some(d) = &default_level
        && !effective.contains(d)
    {
        return invalid(format!(
            "The default effort \"{d}\" is not one of the levels."
        ));
    }
    let custom = match effort.custom {
        None => None,
        Some(map) => {
            let mut clean = BTreeMap::new();
            for (level, value) in map {
                if !effective.contains(&level) {
                    return invalid(format!("\"{level}\" is not one of the effort levels."));
                }
                let value = value.trim();
                if value.is_empty() {
                    return invalid(format!("Enter what to send for the level \"{level}\"."));
                }
                clean.insert(level, value.to_string());
            }
            Some(clean)
        }
    };
    Ok(EffortConfig {
        enabled: effort.enabled,
        levels,
        default_level,
        custom,
    })
}

/// FR-LLM-PV-7: the request format must fit the provider.
fn check_format(provider: &Provider, format: RequestFormat) -> Result<()> {
    let claude_endpoint = provider.kind == ProviderKind::AzureFoundry
        && azure_style(&provider.endpoint) == AzureStyle::Anthropic;
    match format {
        RequestFormat::AnthropicMessages
            if provider.kind != ProviderKind::Anthropic && !claude_endpoint =>
        {
            invalid(
                "The Claude format only works with the Claude API or an Azure endpoint ending in /anthropic.",
            )
        }
        RequestFormat::Responses if provider.kind == ProviderKind::Anthropic => {
            invalid("The Claude API doesn't offer the Responses format.")
        }
        // D-69: Cairn's DeepSeek web search runs on Chat Completions.
        RequestFormat::Responses if provider.kind == ProviderKind::DeepSeek => {
            invalid("DeepSeek models use the Chat Completions format in Cairn.")
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------- providers

fn kind_str(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::AzureFoundry => "azure_foundry",
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::OpenaiCompatible => "openai_compatible",
        ProviderKind::Local => "local",
        ProviderKind::DeepSeek => "deepseek",
    }
}

fn auth_str(auth: AuthMode) -> &'static str {
    match auth {
        AuthMode::ApiKey => "api_key",
        AuthMode::AzureCli => "azure_cli",
        AuthMode::None => "none",
    }
}

fn format_str(format: RequestFormat) -> &'static str {
    match format {
        RequestFormat::ChatCompletions => "chat_completions",
        RequestFormat::Responses => "responses",
        RequestFormat::AnthropicMessages => "anthropic_messages",
    }
}

/// A stored value the schema's CHECK constraints should have prevented.
fn bad_value(column: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        format!("unknown {column}: {value}").into(),
    )
}

const PROVIDER_COLUMNS: &str =
    "id, name, kind, endpoint, api_version, auth, enabled, created_at, updated_at";

/// `has_key` is filled in by the caller (it comes from the secret store, not the database).
fn provider_from_row(r: &rusqlite::Row) -> rusqlite::Result<Provider> {
    let kind: String = r.get(2)?;
    let auth: String = r.get(5)?;
    Ok(Provider {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: match kind.as_str() {
            "azure_foundry" => ProviderKind::AzureFoundry,
            "anthropic" => ProviderKind::Anthropic,
            "openai_compatible" => ProviderKind::OpenaiCompatible,
            "local" => ProviderKind::Local,
            "deepseek" => ProviderKind::DeepSeek,
            other => return Err(bad_value("provider kind", other)),
        },
        endpoint: r.get(3)?,
        api_version: r.get(4)?,
        auth: match auth.as_str() {
            "api_key" => AuthMode::ApiKey,
            "azure_cli" => AuthMode::AzureCli,
            "none" => AuthMode::None,
            other => return Err(bad_value("auth mode", other)),
        },
        enabled: r.get::<_, i64>(6)? != 0,
        has_key: false,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

/// Only providers that use a key can have one (NFR-SEC-1); a failing store counts as "no key".
fn with_key_flag(mut p: Provider, secrets: &dyn SecretStore) -> Provider {
    p.has_key = p.auth == AuthMode::ApiKey && matches!(secrets.get(&p.id), Ok(Some(_)));
    p
}

fn load_provider(conn: &Connection, id: &str) -> Result<Provider> {
    conn.query_row(
        &format!("SELECT {PROVIDER_COLUMNS} FROM providers WHERE id = ?1"),
        [id],
        provider_from_row,
    )
    .optional()?
    .ok_or_else(|| CoreError::NotFound {
        entity: "provider",
        id: id.to_string(),
    })
}

fn get_provider(conn: &Connection, secrets: &dyn SecretStore, id: &str) -> Result<Provider> {
    Ok(with_key_flag(load_provider(conn, id)?, secrets))
}

fn provider_name_taken(conn: &Connection, name: &str, except_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM providers WHERE name = ?1 COLLATE NOCASE AND id <> ?2)",
        params![name, except_id],
        |r| r.get(0),
    )?)
}

fn name_conflict(name: &str) -> CoreError {
    CoreError::Conflict(format!(
        "There is already a provider called \"{name}\". Choose a different name."
    ))
}

/// Providers in card order, with `has_key` read from the secret store.
pub fn list_providers(conn: &Connection, secrets: &dyn SecretStore) -> Result<Vec<Provider>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PROVIDER_COLUMNS} FROM providers ORDER BY position, created_at, id"
    ))?;
    let rows = stmt.query_map([], provider_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(with_key_flag(row?, secrets));
    }
    Ok(out)
}

pub fn create_provider(
    db: &mut Db,
    secrets: &dyn SecretStore,
    input: NewProvider,
) -> Result<Provider> {
    let name = clean_name(&input.name, "provider")?;
    let (endpoint, api_version) = check_provider_fields(
        input.kind,
        input.endpoint.as_deref().unwrap_or(""),
        input.api_version.as_deref(),
        input.auth,
    )?;
    let id = new_id();
    db.tx(|tx| {
        if provider_name_taken(tx, &name, "")? {
            return Err(name_conflict(&name));
        }
        let position: i64 =
            tx.query_row("SELECT COALESCE(MAX(position), 0) + 1 FROM providers", [], |r| {
                r.get(0)
            })?;
        let now = now_iso();
        tx.execute(
            "INSERT INTO providers (id, name, kind, endpoint, api_version, auth, enabled, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8, ?8)",
            params![
                id,
                name,
                kind_str(input.kind),
                endpoint,
                api_version,
                auth_str(input.auth),
                position,
                now
            ],
        )?;
        Ok(())
    })?;
    get_provider(db.conn(), secrets, &id)
}

pub fn update_provider(
    db: &mut Db,
    secrets: &dyn SecretStore,
    id: &str,
    patch: ProviderPatch,
) -> Result<Provider> {
    let current = load_provider(db.conn(), id)?;
    let name = match &patch.name {
        Some(n) => clean_name(n, "provider")?,
        None => current.name.clone(),
    };
    let endpoint = patch.endpoint.as_deref().unwrap_or(&current.endpoint);
    let api_version = match &patch.api_version {
        Some(v) => Some(v.as_str()),
        None => current.api_version.as_deref(),
    };
    let auth = patch.auth.unwrap_or(current.auth);
    let (endpoint, api_version) = check_provider_fields(current.kind, endpoint, api_version, auth)?;
    let enabled = patch.enabled.unwrap_or(current.enabled);
    db.tx(|tx| {
        if provider_name_taken(tx, &name, id)? {
            return Err(name_conflict(&name));
        }
        tx.execute(
            "UPDATE providers SET name = ?2, endpoint = ?3, api_version = ?4, auth = ?5,
                    enabled = ?6, updated_at = ?7 WHERE id = ?1",
            params![
                id,
                name,
                endpoint,
                api_version,
                auth_str(auth),
                enabled,
                now_iso()
            ],
        )?;
        Ok(())
    })?;
    get_provider(db.conn(), secrets, id)
}

/// Clears the default / OCR model settings that point at one of `model_ids` (UI-SET-3).
/// Returns how many settings were cleared. Written directly so `settings.rs` stays unchanged:
/// a setting holds its model id as a JSON string.
fn clear_model_settings(conn: &Connection, model_ids: &[String]) -> Result<usize> {
    let mut cleared = 0;
    for id in model_ids {
        cleared += conn.execute(
            "UPDATE settings SET value = 'null'
             WHERE key IN ('defaultModelId', 'ocrModelId') AND value = ?1",
            [serde_json::to_string(id)?],
        )?;
    }
    Ok(cleared)
}

/// Deletes the provider, its models and its key. Settings that pointed at one of its models
/// (default / OCR model) are cleared; the returned number says how many (so the caller can tell
/// the UI that settings changed too).
pub fn delete_provider(db: &mut Db, secrets: &dyn SecretStore, id: &str) -> Result<usize> {
    load_provider(db.conn(), id)?;
    let cleared = db.tx(|tx| {
        let ids: Vec<String> = tx
            .prepare("SELECT id FROM models WHERE provider_id = ?1")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let cleared = clear_model_settings(tx, &ids)?;
        tx.execute("DELETE FROM models WHERE provider_id = ?1", [id])?;
        tx.execute("DELETE FROM providers WHERE id = ?1", [id])?;
        Ok(cleared)
    })?;
    // After the commit: if the keychain fails here, the provider is gone either way.
    if let Err(e) = secrets.delete(id) {
        tracing::warn!(error = %e, "could not remove the key of a deleted provider");
    }
    Ok(cleared)
}

/// Saves the API key in the secure store (NFR-SEC-1). Refused for providers that don't use a key.
pub fn set_key(conn: &Connection, secrets: &dyn SecretStore, id: &str, key: &str) -> Result<()> {
    let provider = load_provider(conn, id)?;
    if provider.auth != AuthMode::ApiKey {
        return invalid(format!(
            "{} doesn't use an API key. Change its authentication first.",
            provider.name
        ));
    }
    let key = key.trim();
    if key.is_empty() {
        return invalid("Paste the API key first.");
    }
    secrets.set(id, key)
}

pub fn remove_key(conn: &Connection, secrets: &dyn SecretStore, id: &str) -> Result<()> {
    load_provider(conn, id)?;
    secrets.delete(id)
}

// ---------------------------------------------------------------- models

const MODEL_COLUMNS: &str = "id, provider_id, name, label, format, vision, effort_enabled, \
     effort_levels, effort_default, effort_custom, created_at, updated_at, context_size, web_search";

fn json_column<T: serde::de::DeserializeOwned>(
    r: &rusqlite::Row,
    index: usize,
) -> rusqlite::Result<Option<T>> {
    let raw: Option<String> = r.get(index)?;
    raw.map(|s| {
        serde_json::from_str(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(e),
            )
        })
    })
    .transpose()
}

fn model_from_row(r: &rusqlite::Row) -> rusqlite::Result<ModelConfig> {
    let format: String = r.get(4)?;
    Ok(ModelConfig {
        id: r.get(0)?,
        provider_id: r.get(1)?,
        name: r.get(2)?,
        label: r.get(3)?,
        format: match format.as_str() {
            "chat_completions" => RequestFormat::ChatCompletions,
            "responses" => RequestFormat::Responses,
            "anthropic_messages" => RequestFormat::AnthropicMessages,
            other => return Err(bad_value("request format", other)),
        },
        vision: r.get::<_, i64>(5)? != 0,
        effort: EffortConfig {
            enabled: r.get::<_, i64>(6)? != 0,
            levels: json_column(r, 7)?,
            default_level: r.get(8)?,
            custom: json_column(r, 9)?,
        },
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        context_size: r.get(12)?,
        web_search: r.get::<_, i64>(13)? != 0,
    })
}

/// One model (M5: the creation flows read its context size).
pub fn get_model(conn: &Connection, id: &str) -> Result<ModelConfig> {
    conn.query_row(
        &format!("SELECT {MODEL_COLUMNS} FROM models WHERE id = ?1"),
        [id],
        model_from_row,
    )
    .optional()?
    .ok_or_else(|| CoreError::NotFound {
        entity: "model",
        id: id.to_string(),
    })
}

/// Every model of every provider, in provider then model order.
pub fn list_models(conn: &Connection) -> Result<Vec<ModelConfig>> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.provider_id, m.name, m.label, m.format, m.vision, m.effort_enabled,
                m.effort_levels, m.effort_default, m.effort_custom, m.created_at, m.updated_at,
                m.context_size, m.web_search
         FROM models m JOIN providers p ON p.id = m.provider_id
         ORDER BY p.position, p.created_at, p.id, m.position, m.created_at, m.id",
    )?;
    let rows = stmt.query_map([], model_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn label_taken(conn: &Connection, provider_id: &str, label: &str, except_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM models
         WHERE provider_id = ?1 AND label = ?2 COLLATE NOCASE AND id <> ?3)",
        params![provider_id, label, except_id],
        |r| r.get(0),
    )?)
}

fn label_conflict(label: &str) -> CoreError {
    CoreError::Conflict(format!(
        "This provider already has a model labelled \"{label}\". Choose a different label."
    ))
}

/// Writes the effort columns' values: levels and custom as JSON text.
fn effort_json(effort: &EffortConfig) -> Result<(Option<String>, Option<String>)> {
    Ok((
        effort
            .levels
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
        effort
            .custom
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    ))
}

pub fn create_model(db: &mut Db, input: NewModel) -> Result<ModelConfig> {
    let provider = load_provider(db.conn(), &input.provider_id)?;
    let name = clean_name(&input.name, "model")?;
    let label = match input.label.as_deref().map(str::trim) {
        Some(l) if !l.is_empty() => l.to_string(),
        _ => name.clone(),
    };
    check_format(&provider, input.format)?;
    let effort = clean_effort(input.effort.unwrap_or_default())?;
    let (levels, custom) = effort_json(&effort)?;
    let id = new_id();
    db.tx(|tx| {
        if label_taken(tx, &provider.id, &label, "")? {
            return Err(label_conflict(&label));
        }
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM models WHERE provider_id = ?1",
            [&provider.id],
            |r| r.get(0),
        )?;
        let now = now_iso();
        tx.execute(
            "INSERT INTO models (id, provider_id, name, label, format, vision, effort_enabled,
                    effort_levels, effort_default, effort_custom, position, created_at, updated_at,
                    context_size, web_search)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13, ?14)",
            params![
                id,
                provider.id,
                name,
                label,
                format_str(input.format),
                input.vision.unwrap_or(false),
                effort.enabled,
                levels,
                effort.default_level,
                custom,
                position,
                now,
                input.context_size.filter(|&n| n > 0),
                input.web_search.unwrap_or(false)
            ],
        )?;
        Ok(())
    })?;
    get_model(db.conn(), &id)
}

pub fn update_model(db: &mut Db, id: &str, patch: ModelPatch) -> Result<ModelConfig> {
    let current = get_model(db.conn(), id)?;
    let provider = load_provider(db.conn(), &current.provider_id)?;
    let name = match &patch.name {
        Some(n) => clean_name(n, "model")?,
        None => current.name.clone(),
    };
    let label = match patch.label.as_deref().map(str::trim) {
        // An emptied label goes back to the default: the name.
        Some("") => name.clone(),
        Some(l) => l.to_string(),
        None => current.label.clone(),
    };
    let format = patch.format.unwrap_or(current.format);
    if patch.format.is_some() {
        check_format(&provider, format)?;
    }
    let effort = clean_effort(patch.effort.unwrap_or(current.effort))?;
    let vision = patch.vision.unwrap_or(current.vision);
    let web_search = patch.web_search.unwrap_or(current.web_search);
    let context_size = match patch.context_size {
        Some(0) => None,
        Some(n) => Some(n),
        None => current.context_size,
    };
    let (levels, custom) = effort_json(&effort)?;
    db.tx(|tx| {
        if label_taken(tx, &provider.id, &label, id)? {
            return Err(label_conflict(&label));
        }
        tx.execute(
            "UPDATE models SET name = ?2, label = ?3, format = ?4, vision = ?5, effort_enabled = ?6,
                    effort_levels = ?7, effort_default = ?8, effort_custom = ?9, updated_at = ?10,
                    context_size = ?11, web_search = ?12
             WHERE id = ?1",
            params![
                id,
                name,
                label,
                format_str(format),
                vision,
                effort.enabled,
                levels,
                effort.default_level,
                custom,
                now_iso(),
                context_size,
                web_search
            ],
        )?;
        Ok(())
    })?;
    get_model(db.conn(), id)
}

/// FR-LLM-AS-12: turns the *Web search* tick on or off without touching anything else (the
/// assistant removes it when the provider refused or ignored a web search).
pub fn set_web_search(db: &mut Db, id: &str, on: bool) -> Result<ModelConfig> {
    get_model(db.conn(), id)?;
    db.conn().execute(
        "UPDATE models SET web_search = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, on, now_iso()],
    )?;
    get_model(db.conn(), id)
}

/// Deletes the model; settings that pointed at it (default / OCR model) are cleared. Returns how
/// many settings were cleared.
pub fn delete_model(db: &mut Db, id: &str) -> Result<usize> {
    get_model(db.conn(), id)?;
    db.tx(|tx| {
        let cleared = clear_model_settings(tx, &[id.to_string()])?;
        tx.execute("DELETE FROM models WHERE id = ?1", [id])?;
        Ok(cleared)
    })
}

// ---------------------------------------------------------------- sidecar targets

/// The provider, if it is usable now: turned on, and with its key when it needs one.
/// Returns the provider and the key to send (NFR-SEC-1: read just before the call).
fn usable_provider(
    conn: &Connection,
    secrets: &dyn SecretStore,
    provider_id: &str,
) -> Result<(Provider, Option<String>)> {
    let provider = load_provider(conn, provider_id)?;
    if !provider.enabled {
        return invalid(format!(
            "{} is turned off in Settings › AI providers.",
            provider.name
        ));
    }
    let api_key = if provider.auth == AuthMode::ApiKey {
        match secrets.get(&provider.id)? {
            Some(k) => Some(k),
            None => {
                return invalid(format!(
                    "Add an API key for {} in Settings › AI providers.",
                    provider.name
                ));
            }
        }
    } else {
        None
    };
    Ok((provider, api_key))
}

fn target_for(
    provider: Provider,
    api_key: Option<String>,
    model: String,
    format: RequestFormat,
    vision: bool,
    web_search: bool,
    effort: &EffortConfig,
) -> CallTarget {
    CallTarget {
        provider_kind: provider.kind,
        endpoint: provider.endpoint,
        api_version: provider.api_version,
        auth: provider.auth,
        api_key,
        model,
        format,
        vision,
        web_search,
        effort: TargetEffort {
            enabled: effort.enabled,
            levels: effort.levels.clone().unwrap_or_else(|| {
                DEFAULT_EFFORT_LEVELS
                    .iter()
                    .map(|l| l.to_string())
                    .collect()
            }),
            default_level: effort.default_level.clone(),
            custom: effort.custom.clone(),
        },
    }
}

/// The target for a call to `model_id`. Fails with a user-readable `Invalid` if the provider is
/// disabled or needs a key that isn't saved.
pub fn call_target(
    conn: &Connection,
    secrets: &dyn SecretStore,
    model_id: &str,
) -> Result<CallTarget> {
    let model = get_model(conn, model_id)?;
    let (provider, api_key) = usable_provider(conn, secrets, &model.provider_id)?;
    Ok(target_for(
        provider,
        api_key,
        model.name,
        model.format,
        model.vision,
        model.web_search,
        &model.effort,
    ))
}

/// A target for provider-level calls (fetch models): `model` is empty.
pub fn provider_target(
    conn: &Connection,
    secrets: &dyn SecretStore,
    provider_id: &str,
) -> Result<CallTarget> {
    let (provider, api_key) = usable_provider(conn, secrets, provider_id)?;
    let format = match provider.kind {
        ProviderKind::Anthropic => RequestFormat::AnthropicMessages,
        _ => RequestFormat::ChatCompletions,
    };
    Ok(target_for(
        provider,
        api_key,
        String::new(),
        format,
        false,
        false,
        &EffortConfig::default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryStore;

    #[test]
    fn level_to_send_follows_the_worker() {
        let mut e = EffortConfig {
            enabled: true,
            ..EffortConfig::default()
        };
        assert_eq!(e.level_to_send(Some("high")), Some("high".into()));
        assert_eq!(e.level_to_send(None), None);
        e.default_level = Some("low".into());
        assert_eq!(e.level_to_send(None), Some("low".into()));
        assert_eq!(e.level_to_send(Some("medium")), Some("medium".into()));
        e.enabled = false;
        assert_eq!(e.level_to_send(Some("high")), None);
        assert_eq!(e.level_to_send(None), None);
    }

    fn open() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_at(dir.path()).unwrap();
        (dir, db)
    }

    fn new_provider(name: &str, kind: ProviderKind, endpoint: &str, auth: AuthMode) -> NewProvider {
        NewProvider {
            name: name.into(),
            kind,
            endpoint: Some(endpoint.into()),
            api_version: None,
            auth,
        }
    }

    fn azure(db: &mut Db, s: &MemoryStore, name: &str, endpoint: &str) -> Provider {
        create_provider(
            db,
            s,
            new_provider(name, ProviderKind::AzureFoundry, endpoint, AuthMode::ApiKey),
        )
        .unwrap()
    }

    fn new_model(provider_id: &str, name: &str, format: RequestFormat) -> NewModel {
        NewModel {
            provider_id: provider_id.into(),
            name: name.into(),
            label: None,
            format,
            vision: None,
            effort: None,
            context_size: None,
            web_search: None,
        }
    }

    const V1: &str = "https://res.services.ai.azure.com/openai/v1";

    #[test]
    fn azure_style_from_url() {
        assert_eq!(azure_style(V1), AzureStyle::OpenAiV1);
        assert_eq!(azure_style(&format!("{V1}/")), AzureStyle::OpenAiV1);
        assert_eq!(
            azure_style("https://res.services.ai.azure.com/anthropic"),
            AzureStyle::Anthropic
        );
        assert_eq!(
            azure_style("https://res.openai.azure.com/"),
            AzureStyle::Classic
        );
        assert_eq!(azure_style(""), AzureStyle::Classic);
    }

    #[test]
    fn provider_rules() {
        let (_d, mut db) = open();
        let s = MemoryStore::default();
        let p = azure(&mut db, &s, " Work Azure ", V1);
        assert_eq!(p.name, "Work Azure");
        assert!(p.enabled && !p.has_key);

        // Name: required, unique ignoring case.
        let err = create_provider(
            &mut db,
            &s,
            new_provider("  ", ProviderKind::Local, "", AuthMode::None),
        );
        assert!(matches!(err, Err(CoreError::Invalid(_))));
        let err = create_provider(
            &mut db,
            &s,
            new_provider("work azure", ProviderKind::Local, "", AuthMode::None),
        );
        assert!(matches!(err, Err(CoreError::Conflict(_))));

        // Endpoint: required for Azure and OpenAI-compatible, http(s) only.
        for (kind, endpoint) in [
            (ProviderKind::AzureFoundry, ""),
            (ProviderKind::OpenaiCompatible, "  "),
            (ProviderKind::OpenaiCompatible, "ftp://x"),
            (ProviderKind::OpenaiCompatible, "http://"),
            (ProviderKind::Anthropic, "not a url"),
        ] {
            let r = create_provider(
                &mut db,
                &s,
                new_provider("X", kind, endpoint, AuthMode::ApiKey),
            );
            assert!(
                matches!(r, Err(CoreError::Invalid(_))),
                "{kind:?} {endpoint:?}"
            );
        }
        // Defaults: local gets Ollama's address, Claude API stays empty.
        let local = create_provider(
            &mut db,
            &s,
            new_provider("Ollama", ProviderKind::Local, "", AuthMode::None),
        )
        .unwrap();
        assert_eq!(local.endpoint, LOCAL_DEFAULT_ENDPOINT);
        let claude = create_provider(
            &mut db,
            &s,
            new_provider("Claude", ProviderKind::Anthropic, "", AuthMode::ApiKey),
        )
        .unwrap();
        assert_eq!(claude.endpoint, "");

        // Authentication per kind.
        for (kind, auth, ok) in [
            (ProviderKind::AzureFoundry, AuthMode::AzureCli, true),
            (ProviderKind::AzureFoundry, AuthMode::None, false),
            (ProviderKind::Anthropic, AuthMode::AzureCli, false),
            (ProviderKind::Anthropic, AuthMode::None, false),
            (ProviderKind::OpenaiCompatible, AuthMode::AzureCli, false),
            (ProviderKind::OpenaiCompatible, AuthMode::None, true),
            (ProviderKind::Local, AuthMode::ApiKey, true),
            (ProviderKind::Local, AuthMode::AzureCli, false),
        ] {
            assert_eq!(auth_allowed(kind, auth), ok, "{kind:?} {auth:?}");
        }
    }

    #[test]
    fn api_version_only_for_the_older_azure_style() {
        let (_d, mut db) = open();
        let s = MemoryStore::default();
        let mut input = new_provider(
            "Old",
            ProviderKind::AzureFoundry,
            "https://res.openai.azure.com",
            AuthMode::ApiKey,
        );
        input.api_version = Some(" 2025-04-01-preview ".into());
        let p = create_provider(&mut db, &s, input).unwrap();
        assert_eq!(p.api_version.as_deref(), Some("2025-04-01-preview"));

        // Switching to the /openai/v1 style drops the version.
        let p = update_provider(
            &mut db,
            &s,
            &p.id,
            ProviderPatch {
                endpoint: Some(V1.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.api_version, None);

        // Not allowed at all for other kinds.
        let mut input = new_provider("Z", ProviderKind::Local, "", AuthMode::None);
        input.api_version = Some("v1".into());
        assert!(matches!(
            create_provider(&mut db, &s, input),
            Err(CoreError::Invalid(_))
        ));
        // "" clears it.
        let q = update_provider(
            &mut db,
            &s,
            &p.id,
            ProviderPatch {
                endpoint: Some("https://res.openai.azure.com".into()),
                api_version: Some("2024-10-21".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(q.api_version.as_deref(), Some("2024-10-21"));
        let q = update_provider(
            &mut db,
            &s,
            &p.id,
            ProviderPatch {
                api_version: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(q.api_version, None);
    }

    #[test]
    fn update_checks_the_merged_result() {
        let (_d, mut db) = open();
        let s = MemoryStore::default();
        let p = azure(&mut db, &s, "A", V1);
        let other = azure(&mut db, &s, "B", V1);
        let patch = |f: fn(&mut ProviderPatch)| {
            let mut p = ProviderPatch::default();
            f(&mut p);
            p
        };
        // Clearing the endpoint of an Azure provider is refused.
        assert!(
            update_provider(
                &mut db,
                &s,
                &p.id,
                patch(|p| p.endpoint = Some(String::new()))
            )
            .is_err()
        );
        // Renaming onto another provider's name is a conflict; onto its own name (other case) is fine.
        assert!(matches!(
            update_provider(&mut db, &s, &p.id, patch(|p| p.name = Some("b".into()))),
            Err(CoreError::Conflict(_))
        ));
        let renamed =
            update_provider(&mut db, &s, &p.id, patch(|p| p.name = Some("a".into()))).unwrap();
        assert_eq!(renamed.name, "a");
        // Wrong auth for the kind.
        assert!(
            update_provider(&mut db, &s, &p.id, patch(|p| p.auth = Some(AuthMode::None))).is_err()
        );
        // Disabled providers stay listed.
        update_provider(&mut db, &s, &other.id, patch(|p| p.enabled = Some(false))).unwrap();
        let all = list_providers(db.conn(), &s).unwrap();
        assert_eq!(all.len(), 2);
        assert!(!all[1].enabled);
        assert!(matches!(
            update_provider(&mut db, &s, "nope", ProviderPatch::default()),
            Err(CoreError::NotFound { .. })
        ));
    }

    #[test]
    fn keys_live_in_the_secret_store() {
        let (_d, mut db) = open();
        let s = MemoryStore::default();
        let p = azure(&mut db, &s, "A", V1);
        set_key(db.conn(), &s, &p.id, "  sk-secret ").unwrap();
        assert_eq!(s.get(&p.id).unwrap().as_deref(), Some("sk-secret"));
        assert!(list_providers(db.conn(), &s).unwrap()[0].has_key);
        // The key is not in the database.
        let dump: String = db
            .conn()
            .query_row(
                "SELECT group_concat(name || endpoint, ',') FROM providers",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!dump.contains("sk-secret"));
        assert!(set_key(db.conn(), &s, &p.id, "  ").is_err());

        // Switching to Azure CLI hides the key flag but keeps the key.
        let p = update_provider(
            &mut db,
            &s,
            &p.id,
            ProviderPatch {
                auth: Some(AuthMode::AzureCli),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!p.has_key);
        assert!(set_key(db.conn(), &s, &p.id, "x").is_err());
        update_provider(
            &mut db,
            &s,
            &p.id,
            ProviderPatch {
                auth: Some(AuthMode::ApiKey),
                ..Default::default()
            },
        )
        .unwrap();
        remove_key(db.conn(), &s, &p.id).unwrap();
        assert!(!list_providers(db.conn(), &s).unwrap()[0].has_key);
        remove_key(db.conn(), &s, &p.id).unwrap();
    }

    #[test]
    fn model_rules() {
        let (_d, mut db) = open();
        let s = MemoryStore::default();
        let az = azure(&mut db, &s, "Azure", V1);
        let az_claude = azure(
            &mut db,
            &s,
            "Azure Claude",
            "https://r.services.ai.azure.com/anthropic",
        );
        let claude = create_provider(
            &mut db,
            &s,
            new_provider("Claude", ProviderKind::Anthropic, "", AuthMode::ApiKey),
        )
        .unwrap();

        let m = create_model(
            &mut db,
            new_model(&az.id, "  gpt-5  ", RequestFormat::Responses),
        )
        .unwrap();
        assert_eq!((m.name.as_str(), m.label.as_str()), ("gpt-5", "gpt-5"));
        assert!(!m.vision && !m.effort.enabled && m.effort.levels.is_none());

        // Name required; label unique per provider (ignoring case), not across providers.
        assert!(create_model(&mut db, new_model(&az.id, " ", RequestFormat::Responses)).is_err());
        let mut dup = new_model(&az.id, "other", RequestFormat::ChatCompletions);
        dup.label = Some("GPT-5".into());
        assert!(matches!(
            create_model(&mut db, dup),
            Err(CoreError::Conflict(_))
        ));
        let mut ok = new_model(&az_claude.id, "gpt-5", RequestFormat::AnthropicMessages);
        ok.label = Some("gpt-5".into());
        create_model(&mut db, ok).unwrap();

        // Formats must fit the provider.
        assert!(
            create_model(
                &mut db,
                new_model(&az.id, "c", RequestFormat::AnthropicMessages)
            )
            .is_err()
        );
        assert!(
            create_model(
                &mut db,
                new_model(&claude.id, "c", RequestFormat::Responses)
            )
            .is_err()
        );
        create_model(
            &mut db,
            new_model(&claude.id, "c", RequestFormat::AnthropicMessages),
        )
        .unwrap();
        let err = update_model(
            &mut db,
            &m.id,
            ModelPatch {
                format: Some(RequestFormat::AnthropicMessages),
                ..Default::default()
            },
        );
        assert!(matches!(err, Err(CoreError::Invalid(_))));

        // Update: emptied label falls back to the name; vision toggles.
        let u = update_model(
            &mut db,
            &m.id,
            ModelPatch {
                name: Some("gpt-5.1".into()),
                label: Some(String::new()),
                vision: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (u.name.as_str(), u.label.as_str(), u.vision),
            ("gpt-5.1", "gpt-5.1", true)
        );

        // Order: provider order, then model order.
        let names: Vec<_> = list_models(db.conn())
            .unwrap()
            .into_iter()
            .map(|m| m.provider_id)
            .collect();
        assert_eq!(
            names,
            vec![az.id.clone(), az_claude.id.clone(), claude.id.clone()]
        );
    }

    #[test]
    fn effort_rules() {
        let (_d, mut db) = open();
        let s = MemoryStore::default();
        let p = azure(&mut db, &s, "A", V1);
        let with = |effort: EffortConfig| {
            let mut m = new_model(&p.id, "m", RequestFormat::ChatCompletions);
            m.effort = Some(effort);
            m
        };
        let levels = |l: &[&str]| Some(l.iter().map(|s| s.to_string()).collect::<Vec<_>>());

        // Levels must not be empty (or blank, or repeated).
        for bad in [levels(&[]), levels(&["low", " "]), levels(&["low", "LOW"])] {
            let e = EffortConfig {
                enabled: true,
                levels: bad,
                ..Default::default()
            };
            assert!(create_model(&mut db, with(e)).is_err());
        }
        // Default must be one of the levels (the built-ins when none are given).
        let e = EffortConfig {
            enabled: true,
            default_level: Some("max".into()),
            ..Default::default()
        };
        assert!(create_model(&mut db, with(e)).is_err());
        let e = EffortConfig {
            enabled: true,
            levels: levels(&["off", "max"]),
            default_level: Some("max".into()),
            ..Default::default()
        };
        assert!(create_model(&mut db, with(e)).is_ok());
        // Custom keys must be levels; values can't be blank.
        let custom = |k: &str, v: &str| Some(BTreeMap::from([(k.to_string(), v.to_string())]));
        let e = EffortConfig {
            enabled: true,
            custom: custom("max", "1"),
            ..Default::default()
        };
        assert!(create_model(&mut db, with(e)).is_err());
        let e = EffortConfig {
            enabled: true,
            custom: custom("low", " "),
            ..Default::default()
        };
        assert!(create_model(&mut db, with(e)).is_err());

        // A good config round-trips through the database.
        let good = EffortConfig {
            enabled: true,
            levels: levels(&["low", "high"]),
            default_level: Some("low".into()),
            custom: custom("high", "8192"),
        };
        let mut m = with(good.clone());
        m.name = "m2".into();
        let saved = create_model(&mut db, m).unwrap();
        assert_eq!(saved.effort, good);
        assert_eq!(list_models(db.conn()).unwrap().last().unwrap().effort, good);
    }

    #[test]
    fn deleting_clears_settings_keys_and_models() {
        let (_d, mut db) = open();
        let s = MemoryStore::default();
        let a = azure(&mut db, &s, "A", V1);
        let b = azure(&mut db, &s, "B", V1);
        set_key(db.conn(), &s, &a.id, "k").unwrap();
        let ma = create_model(&mut db, new_model(&a.id, "m", RequestFormat::Responses)).unwrap();
        let mb = create_model(&mut db, new_model(&b.id, "m", RequestFormat::Responses)).unwrap();
        let ma2 = create_model(&mut db, new_model(&a.id, "m2", RequestFormat::Responses)).unwrap();
        crate::settings::update(
            &mut db,
            crate::settings::SettingsPatch {
                default_model_id: Some(ma.id.clone()),
                ocr_model_id: Some(mb.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();

        // Deleting a model clears only the settings that pointed at it.
        assert_eq!(delete_model(&mut db, &ma2.id).unwrap(), 0);
        assert_eq!(delete_model(&mut db, &ma.id).unwrap(), 1);
        let st = crate::settings::get(db.conn()).unwrap();
        assert_eq!(st.default_model_id, None);
        assert_eq!(st.ocr_model_id.as_deref(), Some(mb.id.as_str()));

        // Deleting a provider removes its models and key and clears settings.
        crate::settings::update(
            &mut db,
            crate::settings::SettingsPatch {
                default_model_id: Some(mb.id.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        create_model(&mut db, new_model(&a.id, "m3", RequestFormat::Responses)).unwrap();
        assert_eq!(delete_provider(&mut db, &s, &b.id).unwrap(), 2);
        let st = crate::settings::get(db.conn()).unwrap();
        assert_eq!((st.default_model_id, st.ocr_model_id), (None, None));
        assert_eq!(list_models(db.conn()).unwrap().len(), 1);

        assert_eq!(delete_provider(&mut db, &s, &a.id).unwrap(), 0);
        assert_eq!(s.get(&a.id).unwrap(), None);
        assert!(list_models(db.conn()).unwrap().is_empty());
        assert!(list_providers(db.conn(), &s).unwrap().is_empty());
        assert!(matches!(
            delete_provider(&mut db, &s, &a.id),
            Err(CoreError::NotFound { .. })
        ));
    }

    #[test]
    fn call_target_checks_and_fills_in() {
        let (_d, mut db) = open();
        let s = MemoryStore::default();
        let p = azure(&mut db, &s, "Azure", V1);
        let m = create_model(&mut db, new_model(&p.id, "dep", RequestFormat::Responses)).unwrap();

        // No key yet.
        let err = call_target(db.conn(), &s, &m.id).unwrap_err().to_string();
        assert!(err.contains("Add an API key for Azure"), "{err}");
        set_key(db.conn(), &s, &p.id, "sk-1").unwrap();
        let t = call_target(db.conn(), &s, &m.id).unwrap();
        assert_eq!(t.api_key.as_deref(), Some("sk-1"));
        assert_eq!((t.model.as_str(), t.endpoint.as_str()), ("dep", V1));
        assert_eq!(t.effort.levels, DEFAULT_EFFORT_LEVELS);
        assert!(!t.effort.enabled);
        // The key never shows in Debug output (NFR-SEC-3).
        assert!(!format!("{t:?}").contains("sk-1"));

        // Provider-level target: no model, key still sent.
        let t = provider_target(db.conn(), &s, &p.id).unwrap();
        assert_eq!(t.model, "");
        assert_eq!(t.api_key.as_deref(), Some("sk-1"));

        // Switched off.
        update_provider(
            &mut db,
            &s,
            &p.id,
            ProviderPatch {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        let err = call_target(db.conn(), &s, &m.id).unwrap_err().to_string();
        assert!(err.contains("turned off in Settings"), "{err}");
        assert!(provider_target(db.conn(), &s, &p.id).is_err());

        // Azure CLI and keyless providers send no key.
        let cli = create_provider(
            &mut db,
            &s,
            new_provider("Cli", ProviderKind::AzureFoundry, V1, AuthMode::AzureCli),
        )
        .unwrap();
        let mc = create_model(
            &mut db,
            new_model(&cli.id, "d", RequestFormat::ChatCompletions),
        )
        .unwrap();
        assert_eq!(call_target(db.conn(), &s, &mc.id).unwrap().api_key, None);
        assert!(matches!(
            call_target(db.conn(), &s, "nope"),
            Err(CoreError::NotFound { .. })
        ));
    }

    /// FR-DATA-5: providers and models are exported and imported; keys never are.
    #[test]
    fn export_import_keeps_providers_but_not_keys() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut source = Db::open_at(a.path()).unwrap();
        let s = MemoryStore::default();
        let p = azure(&mut source, &s, "Azure", V1);
        set_key(source.conn(), &s, &p.id, "sk-secret").unwrap();
        let effort = EffortConfig {
            enabled: true,
            levels: Some(vec!["low".into(), "high".into()]),
            default_level: Some("high".into()),
            custom: Some(BTreeMap::from([("high".to_string(), "4096".to_string())])),
        };
        let mut nm = new_model(&p.id, "dep", RequestFormat::Responses);
        nm.effort = Some(effort);
        nm.vision = Some(true);
        create_model(&mut source, nm).unwrap();

        let zip = a.path().join("out.zip");
        crate::export::export_to(&source, &zip).unwrap();
        let bytes = std::fs::read(&zip).unwrap();
        assert!(!bytes.windows(9).any(|w| w == b"sk-secret"));

        let mut target = Db::open_at(b.path()).unwrap();
        crate::export::import_from(&mut target, &zip).unwrap();
        let empty = MemoryStore::default(); // another computer: no keys
        let mut expected = list_providers(source.conn(), &s).unwrap();
        expected[0].has_key = false;
        assert_eq!(list_providers(target.conn(), &empty).unwrap(), expected);
        assert_eq!(
            list_models(target.conn()).unwrap(),
            list_models(source.conn()).unwrap()
        );
    }
}

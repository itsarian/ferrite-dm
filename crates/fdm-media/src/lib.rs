use anyhow::{Context as _, Result, bail};
use boa_engine::context::ContextBuilder;
use boa_engine::job::{Job, JobExecutor, SimpleJobExecutor, TimeoutJob};
use boa_engine::{Context, JsArgs, JsNativeError, JsObject, JsResult, JsValue, NativeFunction, Source, js_string};
use fdm_core::FdmConfig;
use fdm_http::build_http_client;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::rc::Rc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsPluginInfo {
    pub uid: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub enabled: bool,
    pub auto_update: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_url: Option<String>,
    pub domains: Vec<String>,
    pub entry_script: PathBuf,
    pub dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginUpdateOutcome {
    pub uid: String,
    pub name: String,
    pub old_version: String,
    pub new_version: String,
}

pub fn compare_semver(a: &str, b: &str) -> std::cmp::Ordering {
    fn parse_parts(v: &str) -> (Vec<u64>, Option<String>) {
        let trimmed = v.trim().trim_start_matches(['v', 'V']);
        let (core, pre) = match trimmed.split_once('-') {
            Some((c, p)) => (c, Some(p.to_ascii_lowercase())),
            None => (trimmed, None),
        };
        let nums = core
            .split('.')
            .map(|seg| seg.trim().parse::<u64>().unwrap_or(0))
            .collect();
        (nums, pre)
    }
    let (pa, pre_a) = parse_parts(a);
    let (pb, pre_b) = parse_parts(b);
    let len = pa.len().max(pb.len());
    for i in 0..len {
        let va = pa.get(i).copied().unwrap_or(0);
        let vb = pb.get(i).copied().unwrap_or(0);
        match va.cmp(&vb) {
            std::cmp::Ordering::Equal => continue,
            non_eq => return non_eq,
        }
    }
    match (pre_a, pre_b) {
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => x.cmp(&y),
        (None, None) => std::cmp::Ordering::Equal,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaVariantOption {
    pub format_id: String,
    pub title: String,
    pub ext: String,
    pub url: String,
    pub audio_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bitrate_kbps: Option<u64>,
    pub codecs: Option<String>,
    pub is_audio_only: bool,
    pub filesize: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleOption {
    pub lang: String,
    pub url: String,
    pub ext: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub url: String,
    pub title: String,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaParseResult {
    pub source_url: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_secs: Option<f64>,
    pub uploader: Option<String>,
    pub is_playlist: bool,
    pub variants: Vec<MediaVariantOption>,
    pub subtitles: Vec<SubtitleOption>,
    pub playlist_entries: Vec<PlaylistEntry>,
    pub extractor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaProbeInfo {
    pub duration_secs: Option<f64>,
    pub format_name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub bit_rate: Option<u64>,
}

thread_local! {
    static TL_HTTP_CLIENT: RefCell<Option<Client>> = const { RefCell::new(None) };
    static TL_TIMERS: RefCell<(u32, std::collections::HashSet<u32>)> = RefCell::new((1, std::collections::HashSet::new()));
    static TL_ALLOW_PYTHON: RefCell<bool> = const { RefCell::new(true) };
    static TL_ALLOW_COOKIES: RefCell<bool> = const { RefCell::new(true) };
    static TL_LOGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

const QT_JS_MS_PARSER_LIB_SHIM: &str = r#"
var console = {
  log: function() { __ferrite_log(Array.prototype.slice.call(arguments).map(String).join(' ')); },
  warn: function() { __ferrite_log('[warn] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); },
  error: function() { __ferrite_log('[error] ' + Array.prototype.slice.call(arguments).map(String).join(' ')); }
};

function setTimeout(cb, ms) {
  return __ferrite_set_timeout(cb, Number(ms) || 0);
}
function clearTimeout(id) {
  __ferrite_clear_timeout(Number(id) || 0);
}

var qtJsDownloadsFactory = {
  create: function() {
    var req = {
      url: '',
      cookies: '',
      postData: null,
      postDataAsUtf8String: null,
      _headers: [],
      _callbacks: [],
      _body: '',
      error: null,
      serializedError: null,
      isNetworkError: false,
      isInternalError: false,
      setCustomHeader: function(k, v) { this._headers.push({ key: String(k), value: String(v) }); },
      dataAsUtf8Text: function() { return this._body; },
      finished: {
        connect: function(cb) { req._callbacks.push(cb); }
      },
      start: function() {
        var self = this;
        var post = self.postDataAsUtf8String !== null ? String(self.postDataAsUtf8String) : '';
        __ferrite_http_fetch(
          String(self.url),
          String(self.cookies || ''),
          JSON.stringify(self._headers),
          post
        ).then(function(res) {
          if (res.ok) {
            self._body = res.body;
            self.cookies = res.cookies || self.cookies;
          } else {
            self.error = res.error || 'HTTP error';
            self.serializedError = self.error;
            self.isNetworkError = !!res.isNetworkError;
          }
          for (var i = 0; i < self._callbacks.length; i++) {
            self._callbacks[i]();
          }
        }, function(err) {
          self.error = String(err);
          self.serializedError = self.error;
          self.isNetworkError = true;
          for (var i = 0; i < self._callbacks.length; i++) {
            self._callbacks[i]();
          }
        });
      }
    };
    return req;
  }
};

function downloadUrlAsUtf8Text(url, cookies, headers, postData) {
  cookies = cookies || '';
  headers = headers || [];
  return new Promise(function(resolve, reject) {
    var download = qtJsDownloadsFactory.create();
    download.url = url;
    download.cookies = cookies;
    if (typeof postData === 'string') {
      download.postDataAsUtf8String = postData;
    }
    for (var i = 0; i < headers.length; i++) {
      download.setCustomHeader(headers[i].key, headers[i].value);
    }
    download.finished.connect(function() {
      var err = download.serializedError || download.error;
      if (err) {
        reject({ error: err, isParseError: !download.isNetworkError && !download.isInternalError });
      } else {
        resolve({ url: url, body: download.dataAsUtf8Text(), cookies: download.cookies });
      }
    });
    download.start();
  });
}

function launchPythonScript(requestId, interactive, script, args) {
  return __ferrite_python_exec(String(script || ''), JSON.stringify(args || [])).then(function(res) {
    if (res.error) {
      return Promise.reject(res);
    }
    return res;
  });
}
"#;

async fn host_http_fetch(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &RefCell<&mut Context>,
) -> JsResult<JsValue> {
    let (url, cookies, headers_json, post_data) = {
        let mut c = ctx.borrow_mut();
        let url = args
            .get_or_undefined(0)
            .to_string(&mut c)?
            .to_std_string_escaped();
        let cookies = args
            .get_or_undefined(1)
            .to_string(&mut c)?
            .to_std_string_escaped();
        let headers_json = args
            .get_or_undefined(2)
            .to_string(&mut c)?
            .to_std_string_escaped();
        let post_data = args
            .get_or_undefined(3)
            .to_string(&mut c)?
            .to_std_string_escaped();
        (url, cookies, headers_json, post_data)
    };

    let client = TL_HTTP_CLIENT
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(Client::new);

    let allow_cookies = TL_ALLOW_COOKIES.with(|b| *b.borrow());
    let mut req = if !post_data.is_empty() {
        client.post(&url).body(post_data)
    } else {
        client.get(&url)
    };
    if allow_cookies && !cookies.is_empty() {
        req = req.header(reqwest::header::COOKIE, cookies.clone());
    }
    if let Ok(hdrs) = serde_json::from_str::<Vec<serde_json::Value>>(&headers_json) {
        for item in hdrs {
            if let (Some(k), Some(v)) = (
                item.get("key").and_then(|s| s.as_str()),
                item.get("value").and_then(|s| s.as_str()),
            ) {
                if !allow_cookies && k.eq_ignore_ascii_case("cookie") {
                    continue;
                }
                req = req.header(k, v);
            }
        }
    }

    let (ok, body, out_cookies, error, is_net) = match req.send().await {
        Ok(resp) => {
            let st = resp.status();
            let set_cookie = resp
                .headers()
                .get_all(reqwest::header::SET_COOKIE)
                .iter()
                .filter_map(|v| v.to_str().ok())
                .map(|s| s.split(';').next().unwrap_or(s))
                .collect::<Vec<_>>()
                .join("; ");
            let final_cookies = if set_cookie.is_empty() {
                cookies
            } else {
                set_cookie
            };
            match resp.text().await {
                Ok(txt) if st.is_success() => (true, txt, final_cookies, String::new(), false),
                Ok(txt) => (
                    false,
                    txt,
                    final_cookies,
                    format!("HTTP status {}", st),
                    false,
                ),
                Err(e) => (false, String::new(), final_cookies, e.to_string(), true),
            }
        }
        Err(e) => (false, String::new(), cookies, e.to_string(), true),
    };

    let mut c = ctx.borrow_mut();
    let obj = JsObject::with_object_proto(c.intrinsics());
    obj.set(js_string!("ok"), ok, false, &mut c)?;
    obj.set(js_string!("body"), js_string!(body), false, &mut c)?;
    obj.set(js_string!("cookies"), js_string!(out_cookies), false, &mut c)?;
    obj.set(js_string!("error"), js_string!(error), false, &mut c)?;
    obj.set(js_string!("isNetworkError"), is_net, false, &mut c)?;
    Ok(obj.into())
}

async fn host_python_exec(
    _this: &JsValue,
    args: &[JsValue],
    ctx: &RefCell<&mut Context>,
) -> JsResult<JsValue> {
    let (script, args_json) = {
        let mut c = ctx.borrow_mut();
        let script = args
            .get_or_undefined(0)
            .to_string(&mut c)?
            .to_std_string_escaped();
        let args_json = args
            .get_or_undefined(1)
            .to_string(&mut c)?
            .to_std_string_escaped();
        (script, args_json)
    };

    let allowed = TL_ALLOW_PYTHON.with(|b| *b.borrow());
    let (exit_code, error, output, error_output) = if !allowed {
        (-1, "Python execution disabled".to_string(), String::new(), String::new())
    } else {
        let cli_args: Vec<String> = serde_json::from_str(&args_json).unwrap_or_default();
        let mut cmd = Command::new("python3");
        cmd.arg("-c").arg(&script);
        for a in cli_args {
            cmd.arg(a);
        }
        match tokio::time::timeout(Duration::from_secs(30), cmd.output()).await {
            Ok(Ok(out)) => {
                let code = out.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
                let err = if out.status.success() {
                    String::new()
                } else {
                    format!("Python exited with {}", code)
                };
                (code, err, stdout, stderr)
            }
            Ok(Err(e)) => (-1, format!("Failed to spawn python3: {e}"), String::new(), String::new()),
            Err(_) => (-1, "Python script timed out".into(), String::new(), String::new()),
        }
    };

    let mut c = ctx.borrow_mut();
    let obj = JsObject::with_object_proto(c.intrinsics());
    obj.set(js_string!("exitCode"), exit_code, false, &mut c)?;
    obj.set(js_string!("error"), js_string!(error), false, &mut c)?;
    obj.set(js_string!("output"), js_string!(output), false, &mut c)?;
    obj.set(js_string!("errorOutput"), js_string!(error_output), false, &mut c)?;
    Ok(obj.into())
}

fn host_log(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let line = args
        .get_or_undefined(0)
        .to_string(ctx)?
        .to_std_string_escaped();
    TL_LOGS.with(|l| l.borrow_mut().push(line));
    Ok(JsValue::undefined())
}

fn host_set_timeout(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let cb = args
        .get_or_undefined(0)
        .as_object()
        .ok_or_else(|| JsNativeError::typ().with_message("setTimeout expects callback function"))?;
    let ms = args
        .get_or_undefined(1)
        .to_number(ctx)
        .unwrap_or(0.0)
        .max(0.0) as u64;

    let timer_id = TL_TIMERS.with(|cell| {
        let mut st = cell.borrow_mut();
        let id = st.0;
        st.0 = id.wrapping_add(1).max(1);
        id
    });
    let job = TimeoutJob::from_duration(
        move |c| {
            let cancelled = TL_TIMERS.with(|cell| cell.borrow_mut().1.remove(&timer_id));
            if !cancelled {
                let _ = cb.call(&JsValue::undefined(), &[], c);
            }
            Ok(JsValue::undefined())
        },
        Duration::from_millis(ms),
    );
    ctx.enqueue_job(Job::TimeoutJob(job));
    Ok(JsValue::from(timer_id as i32))
}

fn host_clear_timeout(_this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = args.get_or_undefined(0).to_i32(ctx).unwrap_or(0) as u32;
    if id > 0 {
        TL_TIMERS.with(|cell| {
            cell.borrow_mut().1.insert(id);
        });
    }
    Ok(JsValue::undefined())
}

pub async fn run_js_parser_script(
    script_source: &str,
    eval_expr: &str,
    config: &FdmConfig,
) -> Result<(serde_json::Value, Vec<String>)> {
    let client = build_http_client(config, &[], None).unwrap_or_else(|_| Client::new());
    let allow_cookies = config.app.plugins_allow_wb_cookies;
    let script_owned = script_source.to_string();
    let expr_owned = eval_expr.to_string();

    tokio::task::spawn_blocking(move || -> Result<(serde_json::Value, Vec<String>)> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let local = tokio::task::LocalSet::new();

        local.block_on(&rt, async move {
            TL_HTTP_CLIENT.with(|c| *c.borrow_mut() = Some(client));
            TL_ALLOW_COOKIES.with(|b| *b.borrow_mut() = allow_cookies);
            TL_LOGS.with(|l| l.borrow_mut().clear());
            TL_TIMERS.with(|t| *t.borrow_mut() = (1, std::collections::HashSet::new()));

            let executor = Rc::new(SimpleJobExecutor::new());
            let mut ctx = ContextBuilder::new()
                .job_executor(executor.clone())
                .build()
                .map_err(|e| anyhow::anyhow!("Boa context init failed: {e}"))?;

            ctx.register_global_callable(
                js_string!("__ferrite_log"),
                1,
                NativeFunction::from_copy_closure(host_log),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            ctx.register_global_callable(
                js_string!("__ferrite_set_timeout"),
                2,
                NativeFunction::from_copy_closure(host_set_timeout),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            ctx.register_global_callable(
                js_string!("__ferrite_clear_timeout"),
                1,
                NativeFunction::from_copy_closure(host_clear_timeout),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            ctx.register_global_callable(
                js_string!("__ferrite_http_fetch"),
                4,
                NativeFunction::from_async_fn(host_http_fetch),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            ctx.register_global_callable(
                js_string!("__ferrite_python_exec"),
                2,
                NativeFunction::from_async_fn(host_python_exec),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;

            ctx.eval(Source::from_bytes(QT_JS_MS_PARSER_LIB_SHIM))
                .map_err(|e| anyhow::anyhow!("Shim eval failed: {e}"))?;
            ctx.eval(Source::from_bytes(&script_owned))
                .map_err(|e| anyhow::anyhow!("Plugin script eval failed: {e}"))?;

            let wrapped = format!(
                "globalThis.__ferrite_result = null; globalThis.__ferrite_error = null;\
                 Promise.resolve().then(function() {{ return ({expr_owned}); }})\
                 .then(function(r) {{ globalThis.__ferrite_result = JSON.stringify(r === undefined ? null : r); }},\
                       function(e) {{ globalThis.__ferrite_error = JSON.stringify(e && e.stack ? String(e) : e); }});"
            );
            ctx.eval(Source::from_bytes(&wrapped))
                .map_err(|e| anyhow::anyhow!("Eval expression failed: {e}"))?;

            executor
                .run_jobs_async(&RefCell::new(&mut ctx))
                .await
                .map_err(|e| anyhow::anyhow!("Async JS job error: {e}"))?;

            let err_val = ctx
                .global_object()
                .get(js_string!("__ferrite_error"), &mut ctx)
                .ok();
            if let Some(v) = err_val {
                if !v.is_null_or_undefined() {
                    let s = v
                        .to_string(&mut ctx)
                        .map(|j| j.to_std_string_escaped())
                        .unwrap_or_else(|_| "JS promise rejected".into());
                    bail!("JS parser rejected: {s}");
                }
            }

            let res_val = ctx
                .global_object()
                .get(js_string!("__ferrite_result"), &mut ctx)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let parsed = if res_val.is_null_or_undefined() {
                serde_json::Value::Null
            } else {
                let s = res_val
                    .to_string(&mut ctx)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .to_std_string_escaped();
                serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s))
            };
            let logs = TL_LOGS.with(|l| l.borrow().clone());
            Ok((parsed, logs))
        })
    })
    .await?
}

#[derive(Debug, Serialize, Deserialize)]
struct PluginManifestFile {
    uid: String,
    name: String,
    version: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    auto_update: bool,
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default = "default_entry")]
    entry: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    update_url: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_entry() -> String {
    "parser.js".into()
}

pub fn list_plugins(plugins_dir: &Path) -> Vec<JsPluginInfo> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(plugins_dir) else {
        return out;
    };
    for ent in rd.flatten() {
        let dir = ent.path();
        if !dir.is_dir() {
            continue;
        }
        let json_path = dir.join("manifest.json");
        if json_path.is_file() {
            if let Ok(raw) = std::fs::read_to_string(&json_path) {
                if let Ok(m) = serde_json::from_str::<PluginManifestFile>(&raw) {
                    let entry_script = dir.join(&m.entry);
                    if entry_script.is_file() {
                        out.push(JsPluginInfo {
                            uid: m.uid,
                            name: m.name,
                            version: m.version,
                            author: m.author,
                            description: m.description,
                            enabled: m.enabled,
                            auto_update: m.auto_update,
                            update_url: m.update_url,
                            domains: m.domains,
                            entry_script,
                            dir,
                        });
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn set_plugin_enabled(plugins_dir: &Path, uid: &str, enabled: bool) -> Result<()> {
    for p in list_plugins(plugins_dir) {
        if p.uid == uid {
            let jp = p.dir.join("manifest.json");
            let raw = std::fs::read_to_string(&jp)?;
            let mut mf: PluginManifestFile = serde_json::from_str(&raw)?;
            mf.enabled = enabled;
            std::fs::write(&jp, serde_json::to_string_pretty(&mf)?)?;
            return Ok(());
        }
    }
    bail!("Plugin '{uid}' not found")
}

pub fn uninstall_plugin(plugins_dir: &Path, uid: &str) -> Result<()> {
    for p in list_plugins(plugins_dir) {
        if p.uid == uid {
            std::fs::remove_dir_all(&p.dir)?;
            return Ok(());
        }
    }
    bail!("Plugin '{uid}' not found")
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else if ty.is_file() {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

pub fn install_plugin_from_path(plugins_dir: &Path, source: &Path) -> Result<JsPluginInfo> {
    install_plugin_from_path_with_options(plugins_dir, source, false)
}

fn verify_plugin_manifest_checksum(
    pkg_dir: &Path,
    mf: &PluginManifestFile,
    ignore_sign: bool,
) -> Result<()> {
    if ignore_sign {
        return Ok(());
    }
    if let Some(ref expected) = mf.sha256 {
        let expected = expected.trim();
        if !expected.is_empty() {
            let entry = pkg_dir.join(&mf.entry);
            let actual =
                fdm_core::hash::compute_file_hash(&entry, fdm_core::HashAlgorithm::Sha256)?;
            if !actual.eq_ignore_ascii_case(expected) {
                bail!(
                    "Plugin '{}' SHA-256 signature mismatch (expected {}, got {})",
                    mf.name,
                    expected,
                    actual
                );
            }
        }
    }
    Ok(())
}

pub fn install_plugin_from_path_with_options(
    plugins_dir: &Path,
    source: &Path,
    ignore_signature: bool,
) -> Result<JsPluginInfo> {
    std::fs::create_dir_all(plugins_dir)?;
    if source.is_dir() {
        let manifest_path = source.join("manifest.json");
        if !manifest_path.is_file() {
            bail!("Directory does not contain manifest.json");
        }
        let raw = std::fs::read_to_string(&manifest_path)?;
        let mf: PluginManifestFile = serde_json::from_str(&raw)?;
        verify_plugin_manifest_checksum(source, &mf, ignore_signature)?;
        let folder_name: String = mf
            .uid
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let dest = plugins_dir.join(if folder_name.is_empty() { "plugin" } else { &folder_name });
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        copy_dir_recursive(source, &dest)?;
        return list_plugins(plugins_dir)
            .into_iter()
            .find(|p| p.uid == mf.uid)
            .context("Installed plugin manifest invalid");
    }

    if !source.is_file() {
        bail!("Plugin source does not exist: {}", source.display());
    }

    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    if ext == "js" {
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("custom-plugin")
            .to_string();
        let uid: String = stem
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
            .collect();
        let dest = plugins_dir.join(&uid);
        std::fs::create_dir_all(&dest)?;
        std::fs::copy(source, dest.join("parser.js"))?;
        let mf = PluginManifestFile {
            uid: uid.clone(),
            name: stem,
            version: "1.0.0".into(),
            author: "User".into(),
            description: "Imported JS parser plugin".into(),
            enabled: true,
            auto_update: false,
            domains: vec![],
            entry: "parser.js".into(),
            sha256: None,
            update_url: None,
        };
        std::fs::write(dest.join("manifest.json"), serde_json::to_string_pretty(&mf)?)?;
        return list_plugins(plugins_dir)
            .into_iter()
            .find(|p| p.uid == uid)
            .context("Failed to load installed JS plugin");
    }

    // Treat .fda / .zip archives via system unzip into staging dir
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let stage = plugins_dir.join(format!(".stage-{}-{}", std::process::id(), ts));
    std::fs::create_dir_all(&stage)?;
    let out = std::process::Command::new("unzip")
        .args(["-q", "-o"])
        .arg(source)
        .arg("-d")
        .arg(&stage)
        .output();
    let ok = matches!(out, Ok(ref o) if o.status.success());
    if !ok {
        let _ = std::fs::remove_dir_all(&stage);
        bail!("Failed to extract plugin archive {}", source.display());
    }

    // Locate manifest.json either at root or one subdirectory deep
    let root_manifest = stage.join("manifest.json");
    let pkg_root = if root_manifest.is_file() {
        stage.clone()
    } else {
        std::fs::read_dir(&stage)?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.is_dir() && p.join("manifest.json").is_file())
            .ok_or_else(|| {
                let _ = std::fs::remove_dir_all(&stage);
                anyhow::anyhow!("Archive does not contain manifest.json")
            })?
    };

    let raw = std::fs::read_to_string(pkg_root.join("manifest.json"))?;
    let mf: PluginManifestFile = serde_json::from_str(&raw)?;
    if let Err(e) = verify_plugin_manifest_checksum(&pkg_root, &mf, ignore_signature) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(e);
    }
    let folder_name: String = mf
        .uid
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let dest = plugins_dir.join(if folder_name.is_empty() { "plugin" } else { &folder_name });
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    copy_dir_recursive(&pkg_root, &dest)?;
    let _ = std::fs::remove_dir_all(&stage);
    list_plugins(plugins_dir)
        .into_iter()
        .find(|p| p.uid == mf.uid)
        .context("Installed archive plugin invalid")
}

#[derive(Debug, Deserialize)]
struct PluginRemoteUpdateManifest {
    version: String,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    script_url: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
}

/// Checks plugins that declare an `update_url` and installs newer versions if available.
/// Only contacts URLs explicitly declared in user-installed plugin manifests (zero telemetry).
pub async fn check_and_update_plugins(
    plugins_dir: &Path,
    config: &FdmConfig,
) -> Result<Vec<PluginUpdateOutcome>> {
    let installed = list_plugins(plugins_dir);
    let updatable: Vec<_> = installed
        .into_iter()
        .filter(|p| p.update_url.as_deref().is_some_and(|u| !u.trim().is_empty()))
        .filter(|p| config.app.update_plugins_automatically || p.auto_update)
        .collect();
    if updatable.is_empty() {
        return Ok(Vec::new());
    }

    let client = build_http_client(config, &[], None)?;
    let mut updated = Vec::new();
    for pl in updatable {
        let Some(ref upd_url) = pl.update_url else {
            continue;
        };
        let resp = match client
            .get(upd_url)
            .timeout(Duration::from_secs(12))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };
        let remote: PluginRemoteUpdateManifest = match resp.json().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if compare_semver(&remote.version, &pl.version) != std::cmp::Ordering::Greater {
            continue;
        }
        let Some(pkg_url) = remote.download_url.or(remote.url).or(remote.script_url) else {
            continue;
        };
        let pkg_resp = match client
            .get(&pkg_url)
            .timeout(Duration::from_secs(20))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };
        let bytes = match pkg_resp.bytes().await {
            Ok(b) => b,
            Err(_) => continue,
        };

        let ext = url::Url::parse(&pkg_url)
            .ok()
            .and_then(|u| {
                u.path_segments()
                    .and_then(|s| s.last().map(str::to_string))
            })
            .and_then(|f| {
                Path::new(&f)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
            })
            .unwrap_or_else(|| "js".into());

        let stage_dir = plugins_dir.join(format!(".upd-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&stage_dir);
        std::fs::create_dir_all(&stage_dir)?;
        let stage_file = stage_dir.join(format!("payload.{ext}"));
        std::fs::write(&stage_file, &bytes)?;

        if !config.app.plugins_dev_ignore_sign {
            if let Some(ref expected) = remote.sha256 {
                let expected = expected.trim();
                if !expected.is_empty() {
                    let actual = fdm_core::hash::compute_file_hash(
                        &stage_file,
                        fdm_core::HashAlgorithm::Sha256,
                    )?;
                    if !actual.eq_ignore_ascii_case(expected) {
                        let _ = std::fs::remove_dir_all(&stage_dir);
                        bail!(
                            "Plugin update '{}' SHA-256 mismatch (expected {}, got {})",
                            pl.name,
                            expected,
                            actual
                        );
                    }
                }
            }
        }

        if ext == "js" {
            std::fs::copy(&stage_file, &pl.entry_script)?;
            let manifest_path = pl.dir.join("manifest.json");
            if let Ok(raw) = std::fs::read_to_string(&manifest_path) {
                if let Ok(mut mf) = serde_json::from_str::<PluginManifestFile>(&raw) {
                    mf.version = remote.version.clone();
                    if remote.sha256.is_some() {
                        mf.sha256 = remote.sha256.clone();
                    }
                    let _ = std::fs::write(&manifest_path, serde_json::to_string_pretty(&mf)?);
                }
            }
        } else {
            let _ = install_plugin_from_path_with_options(
                plugins_dir,
                &stage_file,
                config.app.plugins_dev_ignore_sign,
            )?;
        }
        let _ = std::fs::remove_dir_all(&stage_dir);

        updated.push(PluginUpdateOutcome {
            uid: pl.uid,
            name: pl.name,
            old_version: pl.version,
            new_version: remote.version,
        });
    }
    Ok(updated)
}

pub fn resolve_ffmpeg_path(config: &FdmConfig) -> Option<PathBuf> {
    if let Some(ref p) = config.core.ffmpeg_path {
        if p.exists() {
            return Some(p.clone());
        }
    }
    for cand in [
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ] {
        let p = PathBuf::from(cand);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub fn resolve_ffprobe_path(config: &FdmConfig) -> Option<PathBuf> {
    if let Some(ffmpeg) = resolve_ffmpeg_path(config) {
        if let Some(parent) = ffmpeg.parent() {
            let probe = parent.join("ffprobe");
            if probe.exists() {
                return Some(probe);
            }
        }
    }
    for cand in [
        "/opt/homebrew/bin/ffprobe",
        "/usr/local/bin/ffprobe",
        "/usr/bin/ffprobe",
    ] {
        let p = PathBuf::from(cand);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub fn resolve_yt_dlp_path(config: &FdmConfig) -> Option<PathBuf> {
    if let Some(ref p) = config.core.yt_dlp_path {
        if p.exists() {
            return Some(p.clone());
        }
    }
    for cand in [
        "/opt/homebrew/bin/yt-dlp",
        "/usr/local/bin/yt-dlp",
        "/usr/bin/yt-dlp",
    ] {
        let p = PathBuf::from(cand);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

pub async fn ffprobe_inspect(config: &FdmConfig, input: &Path) -> Result<MediaProbeInfo> {
    let ffprobe = resolve_ffprobe_path(config).context("ffprobe not found")?;
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(input)
        .output()
        .await?;
    if !out.status.success() {
        bail!("ffprobe failed");
    }
    let val: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let fmt = &val["format"];
    let duration_secs = fmt["duration"].as_str().and_then(|s| s.parse::<f64>().ok());
    let bit_rate = fmt["bit_rate"].as_str().and_then(|s| s.parse::<u64>().ok());
    let format_name = fmt["format_name"].as_str().unwrap_or("").to_string();
    let mut width = None;
    let mut height = None;
    let mut video_codec = None;
    let mut audio_codec = None;

    if let Some(streams) = val["streams"].as_array() {
        for st in streams {
            match st["codec_type"].as_str() {
                Some("video") if video_codec.is_none() => {
                    video_codec = st["codec_name"].as_str().map(str::to_string);
                    width = st["width"].as_u64().map(|n| n as u32);
                    height = st["height"].as_u64().map(|n| n as u32);
                }
                Some("audio") if audio_codec.is_none() => {
                    audio_codec = st["codec_name"].as_str().map(str::to_string);
                }
                _ => {}
            }
        }
    }

    Ok(MediaProbeInfo {
        duration_secs,
        format_name,
        width,
        height,
        video_codec,
        audio_codec,
        bit_rate,
    })
}

async fn run_ffmpeg_with_progress<F>(
    ffmpeg: &Path,
    args: &[&str],
    total_duration_secs: Option<f64>,
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(f32) + Send,
{
    let mut child = Command::new(ffmpeg)
        .args(args)
        .args(["-progress", "pipe:1", "-nostats"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdout) = child.stdout.take() {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(val) = line.strip_prefix("out_time_ms=") {
                if let (Ok(us), Some(tot)) = (val.trim().parse::<f64>(), total_duration_secs) {
                    if tot > 0.0 {
                        let pct = ((us / 1_000_000.0) / tot * 100.0).clamp(0.0, 99.9) as f32;
                        on_progress(pct);
                    }
                }
            } else if line.trim() == "progress=end" {
                on_progress(100.0);
            }
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        bail!("ffmpeg exited with {}", status);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mp3QualityOption {
    Cbr(u32),
    Vbr(u8),
}

pub async fn convert_to_mp3<F>(
    config: &FdmConfig,
    input: &Path,
    output: &Path,
    bitrate_kbps: u32,
    on_progress: F,
) -> Result<()>
where
    F: FnMut(f32) + Send,
{
    convert_to_mp3_with_options(
        config,
        input,
        output,
        Mp3QualityOption::Cbr(bitrate_kbps),
        on_progress,
    )
    .await
}

pub async fn convert_to_mp3_with_options<F>(
    config: &FdmConfig,
    input: &Path,
    output: &Path,
    quality: Mp3QualityOption,
    on_progress: F,
) -> Result<()>
where
    F: FnMut(f32) + Send,
{
    let ffmpeg = resolve_ffmpeg_path(config).context("ffmpeg not found")?;
    let probe = ffprobe_inspect(config, input).await.ok();
    let total_dur = probe.and_then(|p| p.duration_secs);
    let in_s = input.to_string_lossy().into_owned();
    let out_s = output.to_string_lossy().into_owned();
    match quality {
        Mp3QualityOption::Cbr(kbps) => {
            let br = format!("{}k", kbps.clamp(64, 320));
            run_ffmpeg_with_progress(
                &ffmpeg,
                &[
                    "-y",
                    "-i",
                    &in_s,
                    "-vn",
                    "-acodec",
                    "libmp3lame",
                    "-b:a",
                    &br,
                    &out_s,
                ],
                total_dur,
                on_progress,
            )
            .await
        }
        Mp3QualityOption::Vbr(q) => {
            let qv = q.min(9).to_string();
            run_ffmpeg_with_progress(
                &ffmpeg,
                &[
                    "-y",
                    "-i",
                    &in_s,
                    "-vn",
                    "-acodec",
                    "libmp3lame",
                    "-q:a",
                    &qv,
                    &out_s,
                ],
                total_dur,
                on_progress,
            )
            .await
        }
    }
}

pub async fn convert_to_mp4<F>(
    config: &FdmConfig,
    input: &Path,
    output: &Path,
    stream_copy: bool,
    on_progress: F,
) -> Result<()>
where
    F: FnMut(f32) + Send,
{
    let ffmpeg = resolve_ffmpeg_path(config).context("ffmpeg not found")?;
    let probe = ffprobe_inspect(config, input).await.ok();
    let total_dur = probe.and_then(|p| p.duration_secs);
    let in_s = input.to_string_lossy().into_owned();
    let out_s = output.to_string_lossy().into_owned();
    if stream_copy {
        run_ffmpeg_with_progress(
            &ffmpeg,
            &["-y", "-i", &in_s, "-c", "copy", "-movflags", "+faststart", &out_s],
            total_dur,
            on_progress,
        )
        .await
    } else {
        run_ffmpeg_with_progress(
            &ffmpeg,
            &[
                "-y",
                "-i",
                &in_s,
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
                "-movflags",
                "+faststart",
                &out_s,
            ],
            total_dur,
            on_progress,
        )
        .await
    }
}

pub async fn merge_av_files(
    config: &FdmConfig,
    video_path: &Path,
    audio_path: &Path,
    output: &Path,
) -> Result<()> {
    let ffmpeg = resolve_ffmpeg_path(config).context("ffmpeg not found")?;
    let status = Command::new(ffmpeg)
        .args(["-y", "-i"])
        .arg(video_path)
        .args(["-i"])
        .arg(audio_path)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(output)
        .output()
        .await?;
    if !status.status.success() {
        bail!(
            "ffmpeg merge failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    Ok(())
}

pub async fn extract_video_thumbnail(
    config: &FdmConfig,
    input: &Path,
    output_jpg: &Path,
    timestamp_secs: f64,
) -> Result<()> {
    let ffmpeg = resolve_ffmpeg_path(config).context("ffmpeg not found")?;
    let ts = format!("{:.2}", timestamp_secs.max(0.0));
    let out = Command::new(ffmpeg)
        .args(["-y", "-ss", &ts, "-i"])
        .arg(input)
        .args(["-vframes", "1", "-q:v", "3"])
        .arg(output_jpg)
        .output()
        .await?;
    if !out.status.success() {
        bail!(
            "Thumbnail extraction failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

pub async fn probe_media_url(
    url: &str,
    plugins_dir: &Path,
    config: &FdmConfig,
) -> Result<MediaParseResult> {
    let parsed_url = url::Url::parse(url)?;
    let host = parsed_url.host_str().unwrap_or("").to_ascii_lowercase();

    // 1. Check matching enabled JS plugins first
    for plugin in list_plugins(plugins_dir) {
        if !plugin.enabled {
            continue;
        }
        let domain_match = plugin.domains.is_empty()
            || plugin
                .domains
                .iter()
                .any(|d| host == *d || host.ends_with(&format!(".{d}")));
        if !domain_match {
            continue;
        }
        if let Ok(script) = std::fs::read_to_string(&plugin.entry_script) {
            let escaped = serde_json::to_string(url)?;
            let expr = format!(
                "(typeof msParser !== 'undefined' && (!msParser.isSupportedSource || msParser.isSupportedSource({escaped}))) ? msParser.parse({escaped}) : null"
            );
            if let Ok((val, _logs)) = run_js_parser_script(&script, &expr, config).await {
                if !val.is_null() {
                    if let Ok(mut parsed) = serde_json::from_value::<MediaParseResult>(val.clone()) {
                        if parsed.extractor.is_empty() {
                            parsed.extractor = format!("plugin:{}", plugin.name);
                        }
                        return Ok(parsed);
                    }
                    // Map looser JS plugin objects cleanly
                    let title = val["title"]
                        .as_str()
                        .unwrap_or("Media Stream")
                        .to_string();
                    let mut variants = Vec::new();
                    if let Some(arr) = val["formats"].as_array().or_else(|| val["variants"].as_array()) {
                        for (idx, item) in arr.iter().enumerate() {
                            if let Some(u) = item["url"].as_str() {
                                variants.push(MediaVariantOption {
                                    format_id: item["format_id"]
                                        .as_str()
                                        .map(str::to_string)
                                        .unwrap_or_else(|| format!("v{idx}")),
                                    title: item["label"]
                                        .as_str()
                                        .or_else(|| item["title"].as_str())
                                        .unwrap_or("Stream")
                                        .to_string(),
                                    ext: item["ext"].as_str().unwrap_or("mp4").to_string(),
                                    url: u.to_string(),
                                    audio_url: item["audio_url"].as_str().map(str::to_string),
                                    width: item["width"].as_u64().map(|n| n as u32),
                                    height: item["height"].as_u64().map(|n| n as u32),
                                    bitrate_kbps: item["bitrate_kbps"].as_u64(),
                                    codecs: item["codecs"].as_str().map(str::to_string),
                                    is_audio_only: item["is_audio_only"].as_bool().unwrap_or(false),
                                    filesize: item["filesize"].as_u64(),
                                });
                            }
                        }
                    }
                    if !variants.is_empty() {
                        return Ok(MediaParseResult {
                            source_url: url.to_string(),
                            title,
                            thumbnail_url: val["thumbnail"].as_str().map(str::to_string),
                            duration_secs: val["duration"].as_f64(),
                            uploader: val["uploader"].as_str().map(str::to_string),
                            is_playlist: false,
                            variants,
                            subtitles: vec![],
                            playlist_entries: vec![],
                            extractor: format!("plugin:{}", plugin.name),
                        });
                    }
                }
            }
        }
    }

    // 2. Direct HLS (.m3u8) / DASH (.mpd) manifests
    let lower = url.to_ascii_lowercase();
    if lower.contains(".m3u8") {
        let client = build_http_client(config, &[], None)?;
        let variants = fdm_hls::inspect_hls_variants(&client, url).await?;
        return Ok(MediaParseResult {
            source_url: url.to_string(),
            title: parsed_url
                .path_segments()
                .and_then(|s| s.last())
                .filter(|s| !s.is_empty())
                .unwrap_or("HLS Stream")
                .to_string(),
            thumbnail_url: None,
            duration_secs: None,
            uploader: None,
            is_playlist: false,
            variants: variants
                .into_iter()
                .enumerate()
                .map(|(i, v)| MediaVariantOption {
                    format_id: format!("hls-{i}"),
                    title: v
                        .resolution
                        .clone()
                        .unwrap_or_else(|| format!("{} kbps", v.bandwidth / 1000)),
                    ext: "mp4".into(),
                    url: v.url,
                    audio_url: None,
                    width: v
                        .resolution
                        .as_deref()
                        .and_then(|r| r.split_once('x')?.0.parse().ok()),
                    height: v
                        .resolution
                        .as_deref()
                        .and_then(|r| r.split_once('x')?.1.parse().ok()),
                    bitrate_kbps: Some(v.bandwidth / 1000),
                    codecs: v.codecs,
                    is_audio_only: false,
                    filesize: None,
                })
                .collect(),
            subtitles: vec![],
            playlist_entries: vec![],
            extractor: "hls".into(),
        });
    }

    if lower.contains(".mpd") {
        let client = build_http_client(config, &[], None)?;
        let xml = client.get(url).send().await?.error_for_status()?.text().await?;
        let reps = fdm_hls::parse_dash_mpd(url, &xml)?;
        return Ok(MediaParseResult {
            source_url: url.to_string(),
            title: "DASH Stream".into(),
            thumbnail_url: None,
            duration_secs: None,
            uploader: None,
            is_playlist: false,
            variants: reps
                .into_iter()
                .map(|r| MediaVariantOption {
                    format_id: r.id,
                    title: format!("{}p ({})", r.height.unwrap_or(0), r.mime_type),
                    ext: "mp4".into(),
                    url: url.to_string(),
                    audio_url: None,
                    width: r.width,
                    height: r.height,
                    bitrate_kbps: Some(r.bandwidth / 1000),
                    codecs: None,
                    is_audio_only: r.mime_type.starts_with("audio/"),
                    filesize: None,
                })
                .collect(),
            subtitles: vec![],
            playlist_entries: vec![],
            extractor: "dash".into(),
        });
    }

    // 3. Optional yt-dlp backend (if user installed yt-dlp)
    if let Some(yt_dlp) = resolve_yt_dlp_path(config) {
        let out = Command::new(yt_dlp)
            .args(["--dump-single-json", "--no-warnings", url])
            .output()
            .await?;
        if out.status.success() {
            let val: serde_json::Value = serde_json::from_slice(&out.stdout)?;
            let title = val["title"].as_str().unwrap_or("Media").to_string();
            let mut entries = Vec::new();
            let mut is_playlist = false;
            if let Some(arr) = val["entries"].as_array() {
                is_playlist = true;
                for e in arr {
                    if let Some(u) = e["webpage_url"].as_str().or_else(|| e["url"].as_str()) {
                        entries.push(PlaylistEntry {
                            url: u.to_string(),
                            title: e["title"].as_str().unwrap_or("Item").to_string(),
                            duration_secs: e["duration"].as_f64(),
                        });
                    }
                }
            }
            let mut variants = Vec::new();
            if let Some(fmts) = val["formats"].as_array() {
                for f in fmts {
                    if let Some(u) = f["url"].as_str() {
                        variants.push(MediaVariantOption {
                            format_id: f["format_id"].as_str().unwrap_or("").to_string(),
                            title: f["format_note"]
                                .as_str()
                                .or_else(|| f["format"].as_str())
                                .unwrap_or("format")
                                .to_string(),
                            ext: f["ext"].as_str().unwrap_or("mp4").to_string(),
                            url: u.to_string(),
                            audio_url: None,
                            width: f["width"].as_u64().map(|n| n as u32),
                            height: f["height"].as_u64().map(|n| n as u32),
                            bitrate_kbps: f["tbr"].as_f64().map(|n| n as u64),
                            codecs: f["vcodec"].as_str().map(str::to_string),
                            is_audio_only: f["vcodec"].as_str() == Some("none"),
                            filesize: f["filesize"].as_u64(),
                        });
                    }
                }
            }
            return Ok(MediaParseResult {
                source_url: url.to_string(),
                title,
                thumbnail_url: val["thumbnail"].as_str().map(str::to_string),
                duration_secs: val["duration"].as_f64(),
                uploader: val["uploader"].as_str().map(str::to_string),
                is_playlist,
                variants,
                subtitles: vec![],
                playlist_entries: entries,
                extractor: "yt-dlp".into(),
            });
        }
    }

    bail!("No installed media parser or yt-dlp handled {}", url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn js_plugin_runtime_supports_async_fetch_and_timers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf).await;
                    let json = r#"{"title":"Demo Clip","media":"https://cdn.example/video.mp4"}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        json.len(),
                        json
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let dir = std::env::temp_dir().join(format!("ferrite_media_js_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let plug_dir = dir.join("example_parser");
        std::fs::create_dir_all(&plug_dir).unwrap();

        std::fs::write(
            plug_dir.join("manifest.json"),
            r#"{"uid":"demo-parser","name":"DemoParser","version":"1.0","author":"Test","domains":["127.0.0.1"],"enabled":true}"#,
        )
        .unwrap();
        std::fs::write(
            plug_dir.join("parser.js"),
            r#"
            var msParser = {
              isSupportedSource: function(url) { return url.indexOf('127.0.0.1') >= 0; },
              parse: function(url) {
                console.log("parsing", url);
                return downloadUrlAsUtf8Text(url).then(function(res) {
                  var data = JSON.parse(res.body);
                  return new Promise(function(resolve) {
                    setTimeout(function() {
                      resolve({
                        title: data.title,
                        formats: [{ format_id: "hd", label: "720p", ext: "mp4", url: data.media, height: 720 }]
                      });
                    }, 15);
                  });
                });
              }
            };
            "#,
        )
        .unwrap();

        let cfg = FdmConfig::default();
        let parsed = probe_media_url(&format!("http://{addr}/watch?v=1"), &dir, &cfg)
            .await
            .unwrap();
        assert_eq!(parsed.title, "Demo Clip");
        assert_eq!(parsed.variants.len(), 1);
        assert_eq!(parsed.variants[0].height, Some(720));
        assert_eq!(parsed.variants[0].url, "https://cdn.example/video.mp4");

        // Disable + verify toggle
        set_plugin_enabled(&dir, "demo-parser", false).unwrap();
        let listed = list_plugins(&dir);
        assert!(!listed[0].enabled);

        srv.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn ffmpeg_probe_mp3_mp4_and_thumbnail_work_if_installed() {
        let cfg = FdmConfig::default();
        let Some(ffmpeg) = resolve_ffmpeg_path(&cfg) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("ferrite_media_ff_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let sample = dir.join("sample.mp4");
        let st = Command::new(&ffmpeg)
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=320x240:rate=24",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:v",
                "mpeg4",
                "-c:a",
                "aac",
            ])
            .arg(&sample)
            .output()
            .await
            .unwrap();
        assert!(st.status.success(), "ffmpeg sample gen failed");

        let info = ffprobe_inspect(&cfg, &sample).await.unwrap();
        assert_eq!(info.width, Some(320));
        assert_eq!(info.height, Some(240));

        let mp3_out = dir.join("audio.mp3");
        convert_to_mp3(&cfg, &sample, &mp3_out, 192, |_| {}).await.unwrap();
        assert!(std::fs::metadata(&mp3_out).unwrap().len() > 100);

        let mp4_out = dir.join("remux.mp4");
        convert_to_mp4(&cfg, &sample, &mp4_out, true, |_| {}).await.unwrap();
        assert!(std::fs::metadata(&mp4_out).unwrap().len() > 100);

        let jpg_out = dir.join("thumb.jpg");
        extract_video_thumbnail(&cfg, &sample, &jpg_out, 0.2).await.unwrap();
        assert!(std::fs::metadata(&jpg_out).unwrap().len() > 100);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn js_plugin_respects_allow_wb_cookies_and_signature_bypass() {
        let seen_cookie = std::sync::Arc::new(std::sync::Mutex::new(Option::<String>::None));
        let seen_srv = std::sync::Arc::clone(&seen_cookie);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let seen_srv = std::sync::Arc::clone(&seen_srv);
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let cookie = req
                        .lines()
                        .find(|l| l.to_ascii_lowercase().starts_with("cookie:"))
                        .map(|l| l.to_string());
                    *seen_srv.lock().unwrap() = cookie;
                    let body = "{}";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{body}"
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });

        let mut cfg = FdmConfig::default();
        cfg.app.plugins_allow_wb_cookies = false;
        let url = format!("http://{addr}/test");
        let _ = run_js_parser_script(
            "",
            &format!("downloadUrlAsUtf8Text({url:?}, 'session=secret123')"),
            &cfg,
        )
        .await
        .unwrap();
        assert!(
            seen_cookie.lock().unwrap().is_none(),
            "cookie must be stripped when plugins_allow_wb_cookies is false"
        );

        let dir = std::env::temp_dir().join(format!("ferrite_media_sig_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src_dir = dir.join("src");
        let dst_dir = dir.join("dst");
        std::fs::create_dir_all(&src_dir).unwrap();
        std::fs::write(src_dir.join("parser.js"), "var msParser = {};").unwrap();
        std::fs::write(
            src_dir.join("manifest.json"),
            r#"{"uid":"sig-test","name":"SigTest","version":"1.0","sha256":"deadbeef"}"#,
        )
        .unwrap();
        assert!(install_plugin_from_path_with_options(&dst_dir, &src_dir, false).is_err());
        assert!(install_plugin_from_path_with_options(&dst_dir, &src_dir, true).is_ok());

        srv.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn semver_and_plugin_auto_update_work_end_to_end() {
        assert_eq!(compare_semver("1.2.0", "1.1.9"), std::cmp::Ordering::Greater);
        assert_eq!(compare_semver("v2.0", "2.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare_semver("1.0.0-alpha", "1.0.0"), std::cmp::Ordering::Less);

        let dir = std::env::temp_dir().join(format!("ferrite_pl_upd_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let plugins_dir = dir.join("plugins");
        let pl_dir = plugins_dir.join("demo");
        std::fs::create_dir_all(&pl_dir).unwrap();
        std::fs::write(pl_dir.join("parser.js"), "var oldv = 1;").unwrap();

        let new_script = b"var oldv = 2;";
        let new_hash = {
            let tmp = dir.join("tmp.js");
            std::fs::write(&tmp, new_script).unwrap();
            fdm_core::hash::compute_file_hash(&tmp, fdm_core::HashAlgorithm::Sha256).unwrap()
        };

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{addr}");
        let manifest_json = format!(
            r#"{{"version":"1.2.0","script_url":"{base_url}/parser.js","sha256":"{new_hash}"}}"#
        );
        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let mj = manifest_json.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first = req.lines().next().unwrap_or("");
                    let (ctype, body): (&str, Vec<u8>) = if first.contains("/manifest.json") {
                        ("application/json", mj.into_bytes())
                    } else {
                        ("application/javascript", new_script.to_vec())
                    };
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                });
            }
        });

        let pl_manifest = format!(
            r#"{{"uid":"demo","name":"Demo","version":"1.0.0","enabled":true,"auto_update":true,"update_url":"{base_url}/manifest.json"}}"#
        );
        std::fs::write(pl_dir.join("manifest.json"), pl_manifest).unwrap();

        let mut cfg = FdmConfig::default();
        cfg.app.update_plugins_automatically = true;
        let updated = check_and_update_plugins(&plugins_dir, &cfg).await.unwrap();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].new_version, "1.2.0");
        assert_eq!(
            std::fs::read_to_string(pl_dir.join("parser.js")).unwrap(),
            "var oldv = 2;"
        );

        srv.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }
}

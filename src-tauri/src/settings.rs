//! ReSession 自有配置（别名、claude 路径覆盖、忙闲阈值等）。
//!
//! 原则（design.md）：原生会话数据只读；用户在 ReSession 里的显式操作
//! （改名、备注、设置）写自己的配置文件 `~/.resession/settings.json`，
//! 别名键带 provider 前缀（`claude:<uuid>`）为未来多 agent 预留。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

fn default_busy_ms() -> u64 {
    4000
}

#[derive(Default, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)] // DTO 跨端：camelCase 勿漏
pub struct Settings {
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    /// 已归档会话 key 列表（`provider:<uuid>`）——原生文件保留，仅列表隐藏
    #[serde(default)]
    pub archived: Vec<String>,
    /// claude 可执行路径覆盖（None = 自动探测）
    #[serde(default)]
    pub claude_path: Option<String>,
    /// 忙闲判定阈值毫秒（输出静默超过视为空闲）
    #[serde(default = "default_busy_ms")]
    pub busy_ms: u64,
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()?;
        Some(PathBuf::from(home).join(".resession").join("settings.json"))
    }

    /// 日志目录：`~/.resession/logs`
    pub fn logs_dir() -> Option<PathBuf> {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()?;
        Some(PathBuf::from(home).join(".resession").join("logs"))
    }

    pub fn load() -> Settings {
        let Some(p) = Self::path() else {
            return Settings::default();
        };
        std::fs::read(&p)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let Some(p) = Self::path() else {
            return Err("no home directory".into());
        };
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&p, data).map_err(|e| e.to_string())
    }
}

pub struct SettingsState(pub Mutex<Settings>);

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约：Settings 必须 camelCase；空对象反序列化吃到 serde 默认值。
    /// key 顺序不是契约（serde_json::Value 内部按字母序），断言一律排序后比较。
    #[test]
    fn settings_keys_camel_case_and_defaults() {
        let v = serde_json::to_value(&Settings::default()).unwrap();
        let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["aliases", "archived", "busyMs", "claudePath"]);
        for k in &keys {
            assert!(!k.contains('_'), "DTO key `{k}` 含蛇形命名");
        }

        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.busy_ms, 4000); // default_busy_ms 生效
        assert_eq!(s.claude_path, None);
        assert!(s.aliases.is_empty());
        assert!(s.archived.is_empty());
    }
}

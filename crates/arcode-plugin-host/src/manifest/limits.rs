//! `[limits]` and clamping to the user's maxima (§3, §5).

use serde::Deserialize;

/// Default guest memory (MiB).
pub const DEFAULT_MEMORY_MB: u32 = 64;
/// Default CPU budget after `host.begin-task` (seconds).
pub const DEFAULT_TASK_SECONDS: u32 = 60;
/// Default for `settings.plugins.max-memory-mb`.
pub const DEFAULT_USER_MAX_MEMORY_MB: u32 = 256;
/// No setting can raise guest memory above this.
pub const HARD_MAX_MEMORY_MB: u32 = 512;
/// Upper bound for `task-seconds`.
pub const MAX_TASK_SECONDS: u32 = 600;

/// `[limits]` exactly as the manifest wrote them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Limits {
    /// Guest linear memory cap in MiB.
    #[serde(default = "default_memory")]
    pub memory_mb: u32,
    /// CPU budget in seconds for calls after `begin-task`.
    #[serde(default = "default_task")]
    pub task_seconds: u32,
}

fn default_memory() -> u32 {
    DEFAULT_MEMORY_MB
}

fn default_task() -> u32 {
    DEFAULT_TASK_SECONDS
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            memory_mb: DEFAULT_MEMORY_MB,
            task_seconds: DEFAULT_TASK_SECONDS,
        }
    }
}

/// The user's maxima (`settings.plugins.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserLimits {
    /// `settings.plugins.max-memory-mb`; itself capped at 512.
    pub max_memory_mb: u32,
    /// Maximum task budget; itself capped at 600.
    pub max_task_seconds: u32,
}

impl Default for UserLimits {
    fn default() -> Self {
        Self {
            max_memory_mb: DEFAULT_USER_MAX_MEMORY_MB,
            max_task_seconds: MAX_TASK_SECONDS,
        }
    }
}

/// Limits after clamping; what the engine enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveLimits {
    /// Bytes of guest linear memory.
    pub memory_bytes: u64,
    /// Seconds of guest CPU after `begin-task`.
    pub task_seconds: u32,
}

impl Limits {
    /// Clamps to `1..=user max` (the user max itself to the hard caps).
    pub fn clamp(&self, user: &UserLimits) -> EffectiveLimits {
        let max_memory = user.max_memory_mb.clamp(1, HARD_MAX_MEMORY_MB);
        let max_task = user.max_task_seconds.clamp(1, MAX_TASK_SECONDS);
        EffectiveLimits {
            memory_bytes: u64::from(self.memory_mb.clamp(1, max_memory)) * 1024 * 1024,
            task_seconds: self.task_seconds.clamp(1, max_task),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    #[test]
    fn defaults_pass_through() {
        let eff = Limits::default().clamp(&UserLimits::default());
        assert_eq!(
            eff,
            EffectiveLimits {
                memory_bytes: 64 * MIB,
                task_seconds: 60
            }
        );
    }

    #[test]
    fn manifest_values_are_clamped_to_user_maxima() {
        let greedy = Limits {
            memory_mb: 4096,
            task_seconds: 10_000,
        };
        assert_eq!(
            greedy.clamp(&UserLimits::default()),
            EffectiveLimits {
                memory_bytes: 256 * MIB,
                task_seconds: 600
            }
        );
        let strict = UserLimits {
            max_memory_mb: 32,
            max_task_seconds: 5,
        };
        assert_eq!(
            greedy.clamp(&strict),
            EffectiveLimits {
                memory_bytes: 32 * MIB,
                task_seconds: 5
            }
        );
    }

    #[test]
    fn user_maxima_are_clamped_to_hard_caps() {
        let user = UserLimits {
            max_memory_mb: 100_000,
            max_task_seconds: 100_000,
        };
        let eff = Limits {
            memory_mb: 100_000,
            task_seconds: 100_000,
        }
        .clamp(&user);
        assert_eq!(
            eff,
            EffectiveLimits {
                memory_bytes: 512 * MIB,
                task_seconds: 600
            }
        );
    }

    #[test]
    fn zero_is_raised_to_one() {
        let eff = Limits {
            memory_mb: 0,
            task_seconds: 0,
        }
        .clamp(&UserLimits {
            max_memory_mb: 0,
            max_task_seconds: 0,
        });
        assert_eq!(
            eff,
            EffectiveLimits {
                memory_bytes: MIB,
                task_seconds: 1
            }
        );
    }
}

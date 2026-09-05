use serde::Deserialize;

use crate::config::PeriodParam;

#[derive(Debug, Clone, Deserialize)]
pub struct SsfStConfig {
    pub period: PeriodParam,
}

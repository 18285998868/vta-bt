use serde::Deserialize;

use crate::config::PeriodParam;

#[derive(Debug, Clone, Deserialize)]
pub struct SsfConfig {
    pub period: PeriodParam,
}

use serde::Deserialize;

use crate::config::PeriodParam;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SsfStConfig {
    pub period: PeriodParam,
}

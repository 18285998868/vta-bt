use serde::Deserialize;

use crate::config::RangeParam;
use crate::error::AppError;

/// 整型网格参数: 单值 / 显式列表 / 等差范围三种形态。
///
/// **为何不用现成的 [`crate::config::PeriodParam`]**: 它只支持单值与等差范围, 而本策略
/// 的研究网格 N{3,5,10,15,20} 与 K{3,5,7,10} **不是等差数列**(N 的间隔 2,5,5,5;
/// K 的间隔 2,2,3), 等差范围会把它悄悄改写成 {3,8,13,18} × {3,5,7,9} —— 那是另一个
/// 网格, 且**静默**发生。形态沿用项目内 [`crate::config::OneOrMany`] 的
/// `#[serde(untagged)]` 先例。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum IntParam {
    /// 单值形态: `left = 10`。
    Single(usize),
    /// 显式列表形态: `right = [3, 5, 7, 10]`。列表内**保持书写顺序**, 不排序不去重。
    List(Vec<usize>),
    /// 等差范围形态: `left = { start = 3, end = 20, step = 5 }`。
    Range(RangeParam<usize>),
}

impl IntParam {
    /// 展开为具体取值列表。
    #[must_use]
    pub fn values(&self) -> Vec<usize> {
        match self {
            Self::Single(v) => vec![*v],
            Self::List(v) => v.clone(),
            Self::Range(r) => (r.start..=r.end).step_by(r.step).collect(),
        }
    }

    /// 语义校验: 每个取值 `>= 1`, 且展开非空。
    ///
    /// # Errors
    /// 含 0 值或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self, name: &str) -> crate::error::Result<()> {
        if let Self::Range(r) = self {
            if r.step == 0 {
                return Err(AppError::Msg(format!("{name}.step 必须 > 0")));
            }
            if r.start > r.end {
                return Err(AppError::Msg(format!(
                    "{name} 起点 {} 不能大于终点 {}",
                    r.start, r.end
                )));
            }
        }
        let vs = self.values();
        if vs.is_empty() {
            return Err(AppError::Msg(format!("{name} 展开为空")));
        }
        if let Some(bad) = vs.iter().find(|v| **v < 1) {
            return Err(AppError::Msg(format!("{name} 必须 >= 1, 当前为 {bad}")));
        }
        Ok(())
    }
}

/// 摆动点突破: `left`(N, 左侧比较根数) × `right`(K, 右侧确认根数) 独立笛卡尔。
/// 经典取值 10/5 与 3/3 皆在网格内。零自由参数之外的过滤项一律不加。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwingFractalConfig {
    /// 摆动点的左侧比较根数 N(其 high/low 须被严格超越)。
    pub left: IntParam,
    /// 摆动点的右侧确认根数 K(其 high/low 须被严格超越), 亦即确认延迟。
    pub right: IntParam,
}

impl SwingFractalConfig {
    /// 展开后的参数组合(按策略的笛卡尔积顺序)。
    #[must_use]
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let ls = self.left.values();
        let rs = self.right.values();
        let mut out = Vec::with_capacity(ls.len() * rs.len());
        for &a in &ls {
            for &b in &rs {
                out.push((a, b));
            }
        }
        out
    }

    /// 语义校验: 范围、步长与网格非空性。
    ///
    /// # Errors
    /// 任一参数越界或展开为空时返回 [`AppError`](crate::error::AppError)。
    pub fn validate(&self) -> crate::error::Result<()> {
        self.left.validate("swing_fractal.left")?;
        self.right.validate("swing_fractal.right")?;
        if self.pairs().is_empty() {
            return Err(AppError::Msg("swing_fractal 参数组合为空".into()));
        }
        Ok(())
    }
}

#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_cartesian_left_right() {
        let c = SwingFractalConfig {
            left: IntParam::Range(RangeParam {
                start: 3,
                end: 10,
                step: 7,
            }),
            right: IntParam::Range(RangeParam {
                start: 2,
                end: 5,
                step: 3,
            }),
        };
        assert_eq!(c.pairs(), vec![(3, 2), (3, 5), (10, 2), (10, 5)]);
    }

    /// **非等差网格** N{3,5,10,15,20} × K{3,5,7,10} 必须恰好 20 组, 且按书写顺序。
    /// 用等差范围会悄悄改写成 {3,8,13,18} × {3,5,7,9} —— 那不是本策略的注册网格。
    #[test]
    fn research_grid_is_exactly_20_in_written_order() {
        let c = SwingFractalConfig {
            left: IntParam::List(vec![3, 5, 10, 15, 20]),
            right: IntParam::List(vec![3, 5, 7, 10]),
        };
        let p = c.pairs();
        assert_eq!(p.len(), 20);
        // 笛卡尔顺序: left 外层(慢变), right 内层(快变), 每档 left 占 4 个槽位。
        assert_eq!(p[0], (3, 3));
        assert_eq!(p[3], (3, 10));
        assert_eq!(p[4], (5, 3));
        assert_eq!(p[15], (15, 10));
        assert_eq!(p[16], (20, 3));
        assert_eq!(p[19], (20, 10));
        assert!(c.validate().is_ok());
    }

    #[test]
    fn single_and_range_forms_agree() {
        assert_eq!(IntParam::Single(10).values(), vec![10]);
        assert_eq!(
            IntParam::Range(RangeParam {
                start: 3,
                end: 10,
                step: 7
            })
            .values(),
            vec![3, 10]
        );
    }

    #[test]
    fn validate_rejects_zero_and_empty() {
        let c = SwingFractalConfig {
            left: IntParam::List(vec![3, 0]),
            right: IntParam::Single(5),
        };
        assert!(c.validate().is_err(), "含 0 值必须被拒");
        let e = SwingFractalConfig {
            left: IntParam::List(vec![]),
            right: IntParam::Single(5),
        };
        assert!(e.validate().is_err(), "空列表必须被拒");
        let z = SwingFractalConfig {
            left: IntParam::Range(RangeParam {
                start: 3,
                end: 3,
                step: 0,
            }),
            right: IntParam::Single(5),
        };
        assert!(z.validate().is_err(), "step=0 必须被拒");
    }

    /// TOML 三种形态都要能反序列化: 单值 / 列表 / 表。
    #[test]
    fn toml_accepts_all_three_forms() {
        let single: SwingFractalConfig = toml::from_str("left = 10\nright = 5").expect("单值形态");
        assert_eq!(single.pairs(), vec![(10, 5)]);

        let list: SwingFractalConfig =
            toml::from_str("left = [3, 10]\nright = [5, 7]").expect("列表形态");
        assert_eq!(list.pairs(), vec![(3, 5), (3, 7), (10, 5), (10, 7)]);

        let range: SwingFractalConfig = toml::from_str(
            "left = { start = 3, end = 20, step = 5 }\nright = { start = 3, end = 10, step = 2 }",
        )
        .expect("范围形态");
        assert_eq!(range.pairs().len(), 16);

        // 拼错的键直接报错, 不被静默忽略
        assert!(toml::from_str::<SwingFractalConfig>("left = 10\nright = 5\nnnn = 1").is_err());
    }
}

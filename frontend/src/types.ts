export interface ApiResponse<T> {
  success: boolean;
  data: T | null;
  error: string | null;
}

export interface MarketTypesData {
  types: string[];
}

export interface PairsData {
  market_type: string;
  pairs: string[];
}

export interface TimeframesData {
  market_type: string;
  pair: string;
  timeframes: string[];
}

export interface Kline {
  time: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

export interface KlinesData {
  market_type: string;
  pair: string;
  timeframe: string;
  count: number;
  klines: Kline[];
}

export interface StrategyMarker {
  time: number;
  side: string;
  /** 成交价；非有限时后端返回 null（NaN/Inf 无法用 JSON 表达）。 */
  price: number | null;
  kind: string;
}

export interface StrategyTrade {
  entry_time: number;
  exit_time: number;
  side: string;
  entry_price: number | null;
  exit_price: number | null;
  qty: number | null;
  pnl: number | null;
  pnl_pct: number | null;
  fixed_pnl: number | null;
  bars_held: number;
}

export interface EquityPoint {
  time: number;
  /** 权益值；非有限时后端返回 null。 */
  value: number | null;
}

export interface StrategySummary {
  num_trades: number;
  /**
   * 以下指标均可为 null：NaN/Inf 是**真实语义**（零波动 ⇒ 夏普无定义；
   * 无亏损 ⇒ 盈利因子无定义）。null 表示"算不出来"，而不是 0。
   */
  win_rate: number | null;
  total_return_pct: number | null;
  sharpe_ratio: number | null;
  sortino_ratio: number | null;
  max_drawdown_pct: number | null;
  profit_factor: number | null;
  expectancy: number | null;
  final_equity: number | null;
  turnover_per_year: number | null;
  fixed_total_return: number | null;
  fixed_sharpe_ratio: number | null;
  fixed_max_drawdown_pct: number | null;
  fixed_expectancy_bps: number | null;
  fixed_profit_factor: number | null;
}

/** 参数网格执行情况（请求几档 / 实跑几档 / 哪些被跳过）。 */
export interface ParamGrid {
  requested: number;
  ran: number;
  params: string[];
  skipped: { params: string; reason: string }[];
}

/** 单档参数网格的回测汇总。 */
export interface RunSummary {
  params: string;
  summary: StrategySummary;
}

export interface StrategyResult {
  strategy: string;
  count: number;
  markers: StrategyMarker[];
  trades: StrategyTrade[];
  equity: EquityPoint[];
  overlays: StrategyOverlay[];
  summary: StrategySummary;
  param_grid: ParamGrid;
  runs: RunSummary[];
}

export interface StrategyOverlay {
  label: string;
  points: EquityPoint[];
}

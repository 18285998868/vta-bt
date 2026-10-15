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
  price: number;
  kind: string;
}

export interface StrategyTrade {
  entry_time: number;
  exit_time: number;
  side: string;
  entry_price: number;
  exit_price: number;
  qty: number;
  pnl: number;
  pnl_pct: number;
  fixed_pnl: number;
  bars_held: number;
}

export interface EquityPoint {
  time: number;
  value: number;
}

export interface StrategySummary {
  num_trades: number;
  win_rate: number;
  total_return_pct: number;
  sharpe_ratio: number;
  sortino_ratio: number;
  max_drawdown_pct: number;
  profit_factor: number;
  expectancy: number;
  final_equity: number;
  turnover_per_year: number;
  fixed_total_return: number;
  fixed_sharpe_ratio: number;
  fixed_max_drawdown_pct: number;
  fixed_expectancy_bps: number;
  fixed_profit_factor: number;
}

export interface StrategyResult {
  strategy: string;
  count: number;
  markers: StrategyMarker[];
  trades: StrategyTrade[];
  equity: EquityPoint[];
  overlays: StrategyOverlay[];
  summary: StrategySummary;
}

export interface StrategyOverlay {
  label: string;
  points: EquityPoint[];
}

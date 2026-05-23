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

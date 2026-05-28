import type { ApiResponse, KlinesData, MarketTypesData, PairsData, TimeframesData } from './types';

const BASE = '/api';

async function fetchJson<T>(url: string): Promise<T> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${res.statusText}`);
  return res.json();
}

export async function fetchMarketTypes(): Promise<string[]> {
  const json = await fetchJson<ApiResponse<MarketTypesData>>(`${BASE}/market-types`);
  if (!json.success || !json.data) throw new Error(json.error ?? 'unknown error');
  return json.data.types;
}

export async function fetchPairs(marketType: string): Promise<string[]> {
  const json = await fetchJson<ApiResponse<PairsData>>(`${BASE}/pairs?market_type=${marketType}`);
  if (!json.success || !json.data) throw new Error(json.error ?? 'unknown error');
  return json.data.pairs;
}

export async function fetchTimeframes(marketType: string, pair: string): Promise<string[]> {
  const json = await fetchJson<ApiResponse<TimeframesData>>(
    `${BASE}/timeframes?market_type=${marketType}&pair=${pair}`,
  );
  if (!json.success || !json.data) throw new Error(json.error ?? 'unknown error');
  return json.data.timeframes;
}

export interface KlinesParams {
  marketType: string;
  pair: string;
  timeframe: string;
  limit?: number;
  before?: number;
  after?: number;
}

export async function fetchKlines(params: KlinesParams): Promise<KlinesData> {
  let url = `${BASE}/klines?market_type=${params.marketType}&pair=${params.pair}&timeframe=${params.timeframe}`;
  if (params.limit !== undefined) url += `&limit=${params.limit}`;
  if (params.before !== undefined) url += `&before=${params.before}`;
  if (params.after !== undefined) url += `&after=${params.after}`;
  const json = await fetchJson<ApiResponse<KlinesData>>(url);
  if (!json.success || !json.data) throw new Error(json.error ?? 'unknown error');
  return json.data;
}

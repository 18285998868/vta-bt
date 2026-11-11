import * as TI from 'technicalindicators';
import type { Kline } from './types';

interface ADXResult { adx: number; pdi: number; mdi: number }
interface ChandelierExitResult { exitLong: number; exitShort: number }
// KST types from TI library are correct at runtime

export type IndicatorCategory = 'main' | 'sub';

export interface ParamDef {
  key: string;
  name: string;
  type: 'number' | 'select';
  default: number;
  min?: number;
  max?: number;
  step?: number;
  options?: { label: string; value: number }[];
}

export type IndicatorParams = Record<string, number>;

export interface OverlaySeries {
  label: string;
  color: string;
  values: (number | null)[];
}

export interface SubSeries {
  label: string;
  color: string;
  type: 'line' | 'histogram';
  values: (number | null)[];
}

export interface SubPane {
  series: SubSeries[];
}

export interface IndicatorResult {
  overlay?: OverlaySeries[];
  sub?: SubPane[];
}

export interface IndicatorDef {
  id: string;
  name: string;
  category: IndicatorCategory;
  description: string;
  defaultParams: IndicatorParams;
  paramDefs: ParamDef[];
  calculate: (klines: Kline[], params: IndicatorParams) => IndicatorResult;
}

export interface ActiveIndicator {
  def: IndicatorDef;
  params: IndicatorParams;
}

const INDICATORS: IndicatorDef[] = [];

function register(ind: IndicatorDef) {
  INDICATORS.push(ind);
}

export function getAllIndicators(): IndicatorDef[] {
  return INDICATORS;
}

export function findIndicator(id: string): IndicatorDef | undefined {
  return INDICATORS.find((i) => i.id === id);
}

export const EMPTY_RESULT: IndicatorResult = {};

export function safeCalc(fn: () => IndicatorResult, label: string): IndicatorResult {
  try { return fn(); } catch (e) { console.error(`Indicator ${label} failed:`, e); return EMPTY_RESULT; }
}

// ─── helpers ───

function closes(klines: Kline[]): number[] {
  return klines.map((k) => k.close);
}
function highs(klines: Kline[]): number[] {
  return klines.map((k) => k.high);
}
function lows(klines: Kline[]): number[] {
  return klines.map((k) => k.low);
}
function volumes(klines: Kline[]): number[] {
  return klines.map((k) => k.volume);
}
/**
 * 把指标库输出对齐到输入 K 线：raw[0] 对应第 firstIndex 根。
 * 未暖机 / NaN / undefined 一律落成 null，避免把“还没有值”画成 0。
 */
function padLeft(
  raw: readonly (number | null | undefined)[],
  firstIndex: number,
  len: number,
): (number | null)[] {
  const out: (number | null)[] = new Array(len).fill(null);
  for (let i = 0; i < raw.length; i++) {
    const idx = firstIndex + i;
    if (idx < 0) continue;
    if (idx >= len) break;
    const v = raw[i];
    if (typeof v === 'number' && Number.isFinite(v)) out[idx] = v;
  }
  return out;
}

/** period 根 high/low 的中点；不足 period 根的位置为 null。 */
function rollingMid(high: number[], low: number[], period: number): (number | null)[] {
  const out: (number | null)[] = new Array(high.length).fill(null);
  for (let i = period - 1; i < high.length; i++) {
    let hi = -Infinity;
    let lo = Infinity;
    for (let j = i - period + 1; j <= i; j++) {
      if (high[j] > hi) hi = high[j];
      if (low[j] < lo) lo = low[j];
    }
    out[i] = (hi + lo) / 2;
  }
  return out;
}


// ───── Main chart overlays ─────

register({
  id: 'sma', name: 'SMA', category: 'main',
  description: 'Simple Moving Average',
  defaultParams: { period: 20 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 20, min: 2, max: 500, step: 1 }],
  calculate(klines, p) {
    const raw = TI.SMA.calculate({ period: p.period, values: closes(klines) });
    return { overlay: [{ label: `SMA(${p.period})`, color: '#fab387', values: padLeft(raw, p.period - 1, klines.length) }] };
  },
});

register({
  id: 'ema', name: 'EMA', category: 'main',
  description: 'Exponential Moving Average',
  defaultParams: { period: 20 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 20, min: 2, max: 500, step: 1 }],
  calculate(klines, p) {
    const raw = TI.EMA.calculate({ period: p.period, values: closes(klines) });
    return { overlay: [{ label: `EMA(${p.period})`, color: '#89b4fa', values: padLeft(raw, p.period - 1, klines.length) }] };
  },
});

register({
  id: 'wma', name: 'WMA', category: 'main',
  description: 'Weighted Moving Average',
  defaultParams: { period: 20 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 20, min: 2, max: 500, step: 1 }],
  calculate(klines, p) {
    const raw = TI.WMA.calculate({ period: p.period, values: closes(klines) });
    return { overlay: [{ label: `WMA(${p.period})`, color: '#f9e2af', values: padLeft(raw, p.period - 1, klines.length) }] };
  },
});

register({
  id: 'bb', name: 'Bollinger Bands', category: 'main',
  description: 'Bollinger Bands (middle, upper, lower)',
  defaultParams: { period: 20, stdDev: 2 },
  paramDefs: [
    { key: 'period', name: '周期', type: 'number', default: 20, min: 2, max: 200, step: 1 },
    { key: 'stdDev', name: '标准差', type: 'number', default: 2, min: 0.5, max: 5, step: 0.5 },
  ],
  calculate(klines, p) {
    const raw = TI.BollingerBands.calculate({ period: p.period, stdDev: p.stdDev, values: closes(klines) });
    const pad = p.period - 1;
    return {
      overlay: [
        { label: `BB中(${p.period})`, color: '#a6e3a1', values: padLeft(raw.map((r) => r.middle), pad, klines.length) },
        { label: 'BB上', color: '#94e2d5', values: padLeft(raw.map((r) => r.upper), pad, klines.length) },
        { label: 'BB下', color: '#94e2d5', values: padLeft(raw.map((r) => r.lower), pad, klines.length) },
      ],
    };
  },
});

register({
  id: 'psar', name: 'Parabolic SAR', category: 'main',
  description: 'Parabolic Stop and Reverse',
  defaultParams: { step: 0.02, max: 0.2 },
  paramDefs: [
    { key: 'step', name: '步长', type: 'number', default: 0.02, min: 0.001, max: 0.1, step: 0.001 },
    { key: 'max', name: '最大值', type: 'number', default: 0.2, min: 0.01, max: 1, step: 0.01 },
  ],
  calculate(klines, p) {
    const raw = TI.PSAR.calculate({ step: p.step, max: p.max, high: highs(klines), low: lows(klines) });
    return { overlay: [{ label: `PSAR(${p.step})`, color: '#f38ba8', values: padLeft(raw, 0, klines.length) }] };
  },
});

register({
  id: 'vwap', name: 'VWAP', category: 'main',
  description: 'Volume Weighted Average Price',
  defaultParams: {},
  paramDefs: [],
  calculate(klines, _p) {
    const raw = TI.VWAP.calculate({ high: highs(klines), low: lows(klines), close: closes(klines), volume: volumes(klines) });
    return { overlay: [{ label: 'VWAP', color: '#cba6f7', values: raw as number[] }] };
  },
});

register({
  id: 'ichimoku', name: 'Ichimoku Cloud', category: 'main',
  description: 'Ichimoku Kinko Hyo',
  defaultParams: { conversion: 9, base: 26, span: 52, displacement: 26 },
  paramDefs: [
    { key: 'conversion', name: '转换线', type: 'number', default: 9, min: 2, max: 100, step: 1 },
    { key: 'base', name: '基准线', type: 'number', default: 26, min: 2, max: 200, step: 1 },
    { key: 'span', name: '迟行线', type: 'number', default: 52, min: 2, max: 200, step: 1 },
    { key: 'displacement', name: '位移', type: 'number', default: 26, min: 1, max: 100, step: 1 },
  ],
  calculate(klines, p) {
    const len = klines.length;
    const high = highs(klines);
    const low = lows(klines);
    const conversion = rollingMid(high, low, p.conversion);
    const base = rollingMid(high, low, p.base);
    const spanBMid = rollingMid(high, low, p.span);
    const spanA: (number | null)[] = new Array(len).fill(null);
    const spanB: (number | null)[] = new Array(len).fill(null);
    // TradingView：转换线/基准线画在当前 bar，先行 A/B 向右位移 displacement 根。
    for (let i = p.displacement; i < len; i++) {
      const src = i - p.displacement;
      const c = conversion[src];
      const b = base[src];
      if (c !== null && b !== null) spanA[i] = (c + b) / 2;
      spanB[i] = spanBMid[src];
    }
    return {
      overlay: [
        { label: `转换线(${p.conversion})`, color: '#89b4fa', values: conversion },
        { label: `基准线(${p.base})`, color: '#f9e2af', values: base },
        { label: '先行A', color: '#a6e3a1', values: spanA },
        { label: '先行B', color: '#f38ba8', values: spanB },
      ],
    };
  },
});

register({
  id: 'keltner', name: 'Keltner Channels', category: 'main',
  description: 'Keltner Channels (middle, upper, lower)',
  defaultParams: { period: 20, multiplier: 2, atrPeriod: 10 },
  paramDefs: [
    { key: 'period', name: 'MA周期', type: 'number', default: 20, min: 2, max: 200, step: 1 },
    { key: 'multiplier', name: '倍数', type: 'number', default: 2, min: 0.5, max: 10, step: 0.5 },
    { key: 'atrPeriod', name: 'ATR周期', type: 'number', default: 10, min: 2, max: 100, step: 1 },
  ],
  calculate(klines, p) {
    const raw = TI.KeltnerChannels.calculate({
      maPeriod: p.period, multiplier: p.multiplier, atrPeriod: p.atrPeriod, useSMA: true,
      high: highs(klines), low: lows(klines), close: closes(klines),
    });
    const pad = Math.max(p.period, p.atrPeriod) - 1;
    return {
      overlay: [
        { label: `KC中(${p.period})`, color: '#a6e3a1', values: padLeft(raw.map((r) => r.middle), pad, klines.length) },
        { label: 'KC上', color: '#94e2d5', values: padLeft(raw.map((r) => r.upper), pad, klines.length) },
        { label: 'KC下', color: '#94e2d5', values: padLeft(raw.map((r) => r.lower), pad, klines.length) },
      ],
    };
  },
});

register({
  id: 'chandelierexit', name: 'Chandelier Exit', category: 'main',
  description: 'Chandelier Exit (long/short stops)',
  defaultParams: { period: 22, multiplier: 3 },
  paramDefs: [
    { key: 'period', name: '周期', type: 'number', default: 22, min: 2, max: 100, step: 1 },
    { key: 'multiplier', name: '倍数', type: 'number', default: 3, min: 0.5, max: 10, step: 0.5 },
  ],
  calculate(klines, p) {
    const raw = TI.ChandelierExit.calculate({
      period: p.period, multiplier: p.multiplier,
      high: highs(klines), low: lows(klines), close: closes(klines),
    }) as unknown as ChandelierExitResult[];
    const pad = p.period;
    return {
      overlay: [
        { label: `CE长(${p.period})`, color: '#a6e3a1', values: padLeft(raw.map((r) => r.exitLong), pad, klines.length) },
        { label: `CE短(${p.period})`, color: '#f38ba8', values: padLeft(raw.map((r) => r.exitShort), pad, klines.length) },
      ],
    };
  },
});

// ───── Sub-chart indicators ─────

register({
  id: 'rsi', name: 'RSI', category: 'sub',
  description: 'Relative Strength Index',
  defaultParams: { period: 14 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 14, min: 2, max: 100, step: 1 }],
  calculate(klines, p) {
    const raw = TI.RSI.calculate({ period: p.period, values: closes(klines) });
    return { sub: [{ series: [{ label: `RSI(${p.period})`, color: '#f9e2af', type: 'line', values: padLeft(raw, p.period, klines.length) }] }] };
  },
});

register({
  id: 'macd', name: 'MACD', category: 'sub',
  description: 'Moving Average Convergence Divergence',
  defaultParams: { fast: 12, slow: 26, signal: 9 },
  paramDefs: [
    { key: 'fast', name: '快线', type: 'number', default: 12, min: 2, max: 100, step: 1 },
    { key: 'slow', name: '慢线', type: 'number', default: 26, min: 2, max: 200, step: 1 },
    { key: 'signal', name: '信号', type: 'number', default: 9, min: 2, max: 100, step: 1 },
  ],
  calculate(klines, p) {
    const raw = TI.MACD.calculate({
      values: closes(klines), fastPeriod: p.fast, slowPeriod: p.slow, signalPeriod: p.signal,
      SimpleMAOscillator: false, SimpleMASignal: false,
    });
    const pad = p.slow - 1;
    return {
      sub: [{
        series: [
          { label: `MACD(${p.fast},${p.slow})`, color: '#89dceb', type: 'line', values: padLeft(raw.map((r) => r.MACD), pad, klines.length) },
          { label: 'Signal', color: '#f9e2af', type: 'line', values: padLeft(raw.map((r) => r.signal), pad, klines.length) },
          { label: 'Histogram', color: '#a6e3a1', type: 'histogram', values: padLeft(raw.map((r) => r.histogram), pad, klines.length) },
        ],
      }],
    };
  },
});

register({
  id: 'stoch', name: 'Stochastic', category: 'sub',
  description: 'Stochastic Oscillator',
  defaultParams: { period: 14, signalPeriod: 3 },
  paramDefs: [
    { key: 'period', name: 'K周期', type: 'number', default: 14, min: 2, max: 100, step: 1 },
    { key: 'signalPeriod', name: 'D周期', type: 'number', default: 3, min: 2, max: 50, step: 1 },
  ],
  calculate(klines, p) {
    const raw = TI.Stochastic.calculate({
      period: p.period, signalPeriod: p.signalPeriod,
      high: highs(klines), low: lows(klines), close: closes(klines),
    });
    const pad = p.period - 1;
    return {
      sub: [{
        series: [
          { label: `%K(${p.period})`, color: '#89dceb', type: 'line', values: padLeft(raw.map((r) => r.k), pad, klines.length) },
          { label: `%D(${p.signalPeriod})`, color: '#f9e2af', type: 'line', values: padLeft(raw.map((r) => r.d), pad, klines.length) },
        ],
      }],
    };
  },
});

register({
  id: 'stochrsi', name: 'Stochastic RSI', category: 'sub',
  description: 'Stochastic RSI Oscillator',
  defaultParams: { rsiPeriod: 14, stochPeriod: 14, kPeriod: 3, dPeriod: 3 },
  paramDefs: [
    { key: 'rsiPeriod', name: 'RSI周期', type: 'number', default: 14, min: 2, max: 100, step: 1 },
    { key: 'stochPeriod', name: '随机周期', type: 'number', default: 14, min: 2, max: 100, step: 1 },
    { key: 'kPeriod', name: 'K周期', type: 'number', default: 3, min: 1, max: 50, step: 1 },
    { key: 'dPeriod', name: 'D周期', type: 'number', default: 3, min: 1, max: 50, step: 1 },
  ],
  calculate(klines, p) {
    const raw = TI.StochasticRSI.calculate({
      rsiPeriod: p.rsiPeriod, stochasticPeriod: p.stochPeriod,
      kPeriod: p.kPeriod, dPeriod: p.dPeriod,
      values: closes(klines),
    });
    // RSI 首值在 rsiPeriod；随后 Stoch/K/D 各消耗 (周期-1) 根。
    const pad = p.rsiPeriod + p.stochPeriod + p.kPeriod + p.dPeriod - 3;
    return {
      sub: [{
        series: [
          { label: 'StochRSI_K', color: '#89dceb', type: 'line', values: padLeft(raw.map((r) => r.k), pad, klines.length) },
          { label: 'StochRSI_D', color: '#f9e2af', type: 'line', values: padLeft(raw.map((r) => r.d), pad, klines.length) },
        ],
      }],
    };
  },
});

register({
  id: 'williamsr', name: 'Williams %R', category: 'sub',
  description: 'Williams Percent Range',
  defaultParams: { period: 14 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 14, min: 2, max: 100, step: 1 }],
  calculate(klines, p) {
    const raw = TI.WilliamsR.calculate({ period: p.period, high: highs(klines), low: lows(klines), close: closes(klines) });
    return { sub: [{ series: [{ label: `%R(${p.period})`, color: '#f9e2af', type: 'line', values: padLeft(raw, p.period - 1, klines.length) }] }] };
  },
});

register({
  id: 'cci', name: 'CCI', category: 'sub',
  description: 'Commodity Channel Index',
  defaultParams: { period: 20 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 20, min: 2, max: 100, step: 1 }],
  calculate(klines, p) {
    const raw = TI.CCI.calculate({ period: p.period, high: highs(klines), low: lows(klines), close: closes(klines) });
    return { sub: [{ series: [{ label: `CCI(${p.period})`, color: '#89dceb', type: 'line', values: padLeft(raw, p.period - 1, klines.length) }] }] };
  },
});

register({
  id: 'adx', name: 'ADX', category: 'sub',
  description: 'Average Directional Index',
  defaultParams: { period: 14 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 14, min: 2, max: 100, step: 1 }],
  calculate(klines, p) {
    const raw = TI.ADX.calculate({ period: p.period, high: highs(klines), low: lows(klines), close: closes(klines) }) as ADXResult[];
    const pad = p.period * 2 - 1;
    return {
      sub: [{
        series: [
          { label: `ADX(${p.period})`, color: '#89dceb', type: 'line', values: padLeft(raw.map((r) => r.adx), pad, klines.length) },
          { label: '+DI', color: '#a6e3a1', type: 'line', values: padLeft(raw.map((r) => r.pdi), pad, klines.length) },
          { label: '-DI', color: '#f38ba8', type: 'line', values: padLeft(raw.map((r) => r.mdi), pad, klines.length) },
        ],
      }],
    };
  },
});

register({
  id: 'atr', name: 'ATR', category: 'sub',
  description: 'Average True Range',
  defaultParams: { period: 14 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 14, min: 2, max: 100, step: 1 }],
  calculate(klines, p) {
    const raw = TI.ATR.calculate({ period: p.period, high: highs(klines), low: lows(klines), close: closes(klines) });
    return { sub: [{ series: [{ label: `ATR(${p.period})`, color: '#f9e2af', type: 'line', values: padLeft(raw, p.period, klines.length) }] }] };
  },
});

register({
  id: 'roc', name: 'ROC', category: 'sub',
  description: 'Rate of Change',
  defaultParams: { period: 12 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 12, min: 2, max: 200, step: 1 }],
  calculate(klines, p) {
    const raw = TI.ROC.calculate({ period: p.period, values: closes(klines) });
    return { sub: [{ series: [{ label: `ROC(${p.period})`, color: '#89dceb', type: 'line', values: padLeft(raw, p.period, klines.length) }] }] };
  },
});

register({
  id: 'kst', name: 'KST', category: 'sub',
  description: 'Know Sure Thing',
  defaultParams: { roc1: 10, roc2: 15, roc3: 20, roc4: 30, sma1: 10, sma2: 10, sma3: 10, sma4: 15, signal: 9 },
  paramDefs: [
    { key: 'roc1', name: 'ROC1', type: 'number', default: 10, min: 2, max: 100, step: 1 },
    { key: 'roc2', name: 'ROC2', type: 'number', default: 15, min: 2, max: 100, step: 1 },
    { key: 'roc3', name: 'ROC3', type: 'number', default: 20, min: 2, max: 100, step: 1 },
    { key: 'roc4', name: 'ROC4', type: 'number', default: 30, min: 2, max: 200, step: 1 },
    { key: 'sma1', name: 'SMA1', type: 'number', default: 10, min: 2, max: 100, step: 1 },
    { key: 'sma2', name: 'SMA2', type: 'number', default: 10, min: 2, max: 100, step: 1 },
    { key: 'sma3', name: 'SMA3', type: 'number', default: 10, min: 2, max: 100, step: 1 },
    { key: 'sma4', name: 'SMA4', type: 'number', default: 15, min: 2, max: 200, step: 1 },
    { key: 'signal', name: '信号', type: 'number', default: 9, min: 2, max: 100, step: 1 },
  ],
  calculate(klines, p) {
    const raw = TI.KST.calculate({
      ROCPer1: p.roc1, ROCPer2: p.roc2, ROCPer3: p.roc3, ROCPer4: p.roc4,
      SMAROCPer1: p.sma1, SMAROCPer2: p.sma2, SMAROCPer3: p.sma3, SMAROCPer4: p.sma4,
      signalPeriod: p.signal, values: closes(klines),
    });
    const pad = Math.max(
      p.roc1 + p.sma1 - 1,
      p.roc2 + p.sma2 - 1,
      p.roc3 + p.sma3 - 1,
      p.roc4 + p.sma4 - 1,
    );
    return {
      sub: [{
        series: [
          { label: 'KST', color: '#89dceb', type: 'line', values: padLeft(raw.map((r) => r.kst), pad, klines.length) },
          // 库输出的前 signalPeriod-1 个 signal 是 undefined：padLeft 会把它们留成 null。
          { label: 'Signal', color: '#f9e2af', type: 'line', values: padLeft(raw.map((r) => r.signal), pad, klines.length) },
        ],
      }],
    };
  },
});

register({
  id: 'trix', name: 'TRIX', category: 'sub',
  description: 'Triple Exponential Average',
  defaultParams: { period: 15, signal: 9 },
  paramDefs: [
    { key: 'period', name: '周期', type: 'number', default: 15, min: 2, max: 100, step: 1 },
    { key: 'signal', name: '信号', type: 'number', default: 9, min: 2, max: 50, step: 1 },
  ],
  calculate(klines, p) {
    const raw = TI.TRIX.calculate({ period: p.period, values: closes(klines) });
    const trixPad = p.period * 3 - 2;
    const trix = padLeft(raw, trixPad, klines.length);
    const sigRaw = TI.SMA.calculate({ period: p.signal, values: raw });
    const sig = padLeft(sigRaw, trixPad + p.signal - 1, klines.length);
    return { sub: [{ series: [
      { label: `TRIX(${p.period})`, color: '#89dceb', type: 'line', values: trix },
      { label: 'Signal', color: '#f9e2af', type: 'line', values: sig },
    ] }] };
  },
});

register({
  id: 'obv', name: 'OBV', category: 'sub',
  description: 'On Balance Volume',
  defaultParams: {}, paramDefs: [],
  calculate(klines, _p) {
    const raw = TI.OBV.calculate({ close: closes(klines), volume: volumes(klines) });
    return { sub: [{ series: [{ label: 'OBV', color: '#94e2d5', type: 'line', values: padLeft(raw, 1, klines.length) }] }] };
  },
});

register({
  id: 'adl', name: 'ADL', category: 'sub',
  description: 'Accumulation Distribution Line',
  defaultParams: {}, paramDefs: [],
  calculate(klines, _p) {
    const raw = TI.ADL.calculate({ high: highs(klines), low: lows(klines), close: closes(klines), volume: volumes(klines) });
    return { sub: [{ series: [{ label: 'ADL', color: '#89dceb', type: 'line', values: raw }] }] };
  },
});

register({
  id: 'mfi', name: 'MFI', category: 'sub',
  description: 'Money Flow Index',
  defaultParams: { period: 14 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 14, min: 2, max: 100, step: 1 }],
  calculate(klines, p) {
    const raw = TI.MFI.calculate({ period: p.period, high: highs(klines), low: lows(klines), close: closes(klines), volume: volumes(klines) });
    return { sub: [{ series: [{ label: `MFI(${p.period})`, color: '#cba6f7', type: 'line', values: padLeft(raw, p.period + 1, klines.length) }] }] };
  },
});

register({
  id: 'forceindex', name: 'Force Index', category: 'sub',
  description: 'Force Index',
  defaultParams: { period: 13 },
  paramDefs: [{ key: 'period', name: '周期', type: 'number', default: 13, min: 2, max: 100, step: 1 }],
  calculate(klines, p) {
    const raw = TI.ForceIndex.calculate({ period: p.period, close: closes(klines), volume: volumes(klines) });
    return { sub: [{ series: [{ label: `FI(${p.period})`, color: '#f9e2af', type: 'line', values: padLeft(raw, p.period, klines.length) }] }] };
  },
});

register({
  id: 'awesome', name: 'Awesome Oscillator', category: 'sub',
  description: 'Awesome Oscillator',
  defaultParams: { fast: 5, slow: 34 },
  paramDefs: [
    { key: 'fast', name: '快线', type: 'number', default: 5, min: 2, max: 50, step: 1 },
    { key: 'slow', name: '慢线', type: 'number', default: 34, min: 2, max: 200, step: 1 },
  ],
  calculate(klines, p) {
    const raw = TI.AwesomeOscillator.calculate({ fastPeriod: p.fast, slowPeriod: p.slow, high: highs(klines), low: lows(klines) });
    return { sub: [{ series: [{ label: `AO(${p.fast},${p.slow})`, color: '#a6e3a1', type: 'histogram', values: padLeft(raw, p.slow - 1, klines.length) }] }] };
  },
});

register({
  id: 'volume', name: 'Volume', category: 'sub',
  description: '成交量',
  defaultParams: {}, paramDefs: [],
  calculate(klines, _p) {
    return { sub: [{ series: [{ label: 'Volume', color: '#94e2d5', type: 'histogram', values: [...volumes(klines)] }] }] };
  },
});

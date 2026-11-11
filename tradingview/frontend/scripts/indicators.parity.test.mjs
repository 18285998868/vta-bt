/**
 * 指标 parity 测试。
 *
 * 口径：TradingView / Pine 的首有效 bar、null 暖机语义，以及回放前缀不变性。
 * 运行：npm test（Node 直接 strip TS 类型后 import src/indicators.ts）。
 */
import assert from 'node:assert/strict';
import test from 'node:test';

import { findIndicator, getAllIndicators } from '../src/indicators.ts';

const N = 400;
const PREFIX = 280;

function makeKlines(n = N) {
  return Array.from({ length: n }, (_, i) => {
    const base = 100 + Math.sin(i / 9) * 8 + Math.cos(i / 31) * 4 + i * 0.01;
    return {
      time: Date.UTC(2024, 0, 1) + i * 3_600_000,
      open: base - 0.5,
      high: base + 1 + Math.sin(i / 5) * 0.3,
      low: base - 1 - Math.cos(i / 7) * 0.3,
      close: base + 0.2 * Math.sin(i / 3),
      volume: 1000 + ((i * 37) % 500),
    };
  });
}

const KLINES = makeKlines();
const CLOSE = KLINES.map((k) => k.close);

function calc(id, klines = KLINES, overrides = {}) {
  const def = findIndicator(id);
  assert.ok(def, `missing indicator: ${id}`);
  return def.calculate(klines, { ...def.defaultParams, ...overrides });
}

function seriesOf(result) {
  return [
    ...(result.overlay ?? []),
    ...(result.sub ?? []).flatMap((pane) => pane.series),
  ];
}

function firstFinite(values) {
  return values.findIndex((v) => v !== null && v !== undefined && Number.isFinite(v));
}

// 默认参数、N=400 时的首有效下标；多 series 按输出顺序。
const EXPECTED_FIRST = {
  sma: [19],
  ema: [19],
  wma: [19],
  bb: [19, 19, 19],
  psar: [0],
  vwap: [0],
  ichimoku: [8, 25, 51, 77],
  keltner: [19, 19, 19],
  chandelierexit: [22, 22],
  rsi: [14],
  macd: [25, 33, 33],
  stoch: [13, 15],
  stochrsi: [31, 31],
  williamsr: [13],
  cci: [19],
  adx: [27, 27, 27],
  atr: [14],
  roc: [12],
  kst: [44, 52],
  trix: [43, 51],
  obv: [1],
  adl: [0],
  mfi: [15],
  forceindex: [13],
  awesome: [33],
  volume: [0],
};

test('指标清单与首有效下标符合 Pine 暖机口径', () => {
  const defs = getAllIndicators();
  assert.equal(defs.length, 26);
  assert.deepEqual(defs.map((d) => d.id).sort(), Object.keys(EXPECTED_FIRST).sort());

  for (const [id, expected] of Object.entries(EXPECTED_FIRST)) {
    const values = seriesOf(calc(id));
    assert.equal(values.length, expected.length, `${id}: series count`);
    values.forEach((series, i) => {
      const { values: data, label } = series;
      assert.equal(data.length, N, `${id}/${label}: length`);
      assert.equal(firstFinite(data), expected[i], `${id}/${label}: first valid`);

      for (let j = 0; j < data.length; j++) {
        const v = data[j];
        assert.ok(
          v === null || (typeof v === 'number' && Number.isFinite(v)),
          `${id}/${label}: bad value at ${j}: ${v}`,
        );
        if (j >= expected[i]) {
          assert.ok(v !== null && v !== undefined, `${id}/${label}: unexpected null at ${j}`);
        }
      }
    });
  }
});

test('指标只用当前及历史 bar：完整序列与前缀在重叠区间一致', () => {
  const prefix = KLINES.slice(0, PREFIX);
  for (const def of getAllIndicators()) {
    const full = seriesOf(def.calculate(KLINES, { ...def.defaultParams }));
    const pre = seriesOf(def.calculate(prefix, { ...def.defaultParams }));
    assert.equal(full.length, pre.length, `${def.id}: series count`);

    full.forEach((series, s) => {
      const a = series.values;
      const b = pre[s].values;
      assert.equal(b.length, PREFIX, `${def.id}/${series.label}: prefix length`);
      for (let i = 0; i < PREFIX; i++) {
        const x = a[i];
        const y = b[i];
        const xf = x !== null && x !== undefined && Number.isFinite(x);
        const yf = y !== null && y !== undefined && Number.isFinite(y);
        assert.equal(yf, xf, `${def.id}/${series.label}: nullness at ${i}`);
        if (xf) {
          assert.ok(
            Math.abs(x - y) <= 1e-8,
            `${def.id}/${series.label}: prefix mismatch at ${i}: ${x} vs ${y}`,
          );
        }
      }
    });
  }
});

function smaRef(values, period) {
  const out = new Array(values.length).fill(null);
  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum += values[i];
    if (i >= period) sum -= values[i - period];
    if (i >= period - 1) out[i] = sum / period;
  }
  return out;
}

function emaRef(values, period) {
  const out = new Array(values.length).fill(null);
  const alpha = 2 / (period + 1);
  let prev = null;
  for (let i = 0; i < values.length; i++) {
    if (i === period - 1) {
      let sum = 0;
      for (let j = 0; j < period; j++) sum += values[j];
      prev = sum / period;
      out[i] = prev;
    } else if (i >= period) {
      prev = alpha * values[i] + (1 - alpha) * prev;
      out[i] = prev;
    }
  }
  return out;
}

function wilderRmaRef(values, period) {
  const out = new Array(values.length).fill(null);
  let prev = null;
  for (let i = 0; i < values.length; i++) {
    if (i === period - 1) {
      let sum = 0;
      for (let j = 0; j < period; j++) sum += values[j];
      prev = sum / period;
      out[i] = prev;
    } else if (i >= period) {
      prev = (prev * (period - 1) + values[i]) / period;
      out[i] = prev;
    }
  }
  return out;
}

function rsiRef(values, period) {
  const out = new Array(values.length).fill(null);
  let gainSum = 0;
  let lossSum = 0;
  for (let i = 1; i <= period; i++) {
    const diff = values[i] - values[i - 1];
    if (diff > 0) gainSum += diff;
    else lossSum -= diff;
  }
  let avgGain = gainSum / period;
  let avgLoss = lossSum / period;
  out[period] = avgLoss === 0 ? 100 : avgGain === 0 ? 0 : 100 - 100 / (1 + avgGain / avgLoss);
  for (let i = period + 1; i < values.length; i++) {
    const diff = values[i] - values[i - 1];
    const gain = diff > 0 ? diff : 0;
    const loss = diff < 0 ? -diff : 0;
    avgGain = (avgGain * (period - 1) + gain) / period;
    avgLoss = (avgLoss * (period - 1) + loss) / period;
    out[i] = avgLoss === 0 ? 100 : avgGain === 0 ? 0 : 100 - 100 / (1 + avgGain / avgLoss);
  }
  return out;
}

test('SMA / EMA / RSI / ATR 与独立参考实现一致', () => {
  const close = CLOSE;

  const sma = seriesOf(calc('sma'))[0].values;
  const smaExpected = smaRef(close, 20);
  const ema = seriesOf(calc('ema'))[0].values;
  const emaExpected = emaRef(close, 20);
  for (let i = 0; i < N; i++) {
    if (sma[i] !== null) assert.ok(Math.abs(sma[i] - smaExpected[i]) < 1e-9, `SMA ${i}`);
    if (ema[i] !== null) assert.ok(Math.abs(ema[i] - emaExpected[i]) < 1e-9, `EMA ${i}`);
  }

  // technicalindicators 的 RSI 输出保留 2 位小数。
  const rsi = seriesOf(calc('rsi'))[0].values;
  const rsiExpected = rsiRef(close, 14);
  for (let i = 0; i < N; i++) {
    if (rsi[i] !== null) assert.ok(Math.abs(rsi[i] - rsiExpected[i]) <= 0.0051, `RSI ${i}`);
  }

  const tr = new Array(N).fill(0);
  tr[0] = KLINES[0].high - KLINES[0].low;
  for (let i = 1; i < N; i++) {
    tr[i] = Math.max(
      KLINES[i].high - KLINES[i].low,
      Math.abs(KLINES[i].high - close[i - 1]),
      Math.abs(KLINES[i].low - close[i - 1]),
    );
  }
  const atr = seriesOf(calc('atr'))[0].values;
  // TrueRange 从第 1 根才有值；WEMA/RMA 用 TR[1..period] 做 seed。
  const atrExpected = wilderRmaRef(tr.slice(1), 14);
  for (let i = 14; i < N; i++) {
    assert.ok(Math.abs(atr[i] - atrExpected[i - 1]) < 1e-9, `ATR ${i}`);
  }
});

test('MACD 三条线共享同一时间轴，暖机期是 null 而不是 0', () => {
  const [macd, signal, hist] = seriesOf(calc('macd')).map((s) => s.values);
  for (let i = 25; i < 33; i++) {
    assert.equal(signal[i], null, `signal warmup ${i}`);
    assert.equal(hist[i], null, `hist warmup ${i}`);
  }

  const fast = emaRef(CLOSE, 12);
  const slow = emaRef(CLOSE, 26);
  for (let i = 25; i < N; i++) {
    assert.ok(Math.abs(macd[i] - (fast[i] - slow[i])) < 1e-9, `MACD ${i}`);
  }

  // Signal 是对已对齐 MACD 序列再取 9 周期 EMA。
  const macdFrom25 = macd.slice(25);
  const sigFrom25 = emaRef(macdFrom25, 9);
  for (let i = 33; i < N; i++) {
    assert.ok(Math.abs(signal[i] - sigFrom25[i - 25]) < 1e-8, `signal ${i}`);
    assert.ok(Math.abs(hist[i] - (macd[i] - signal[i])) < 1e-8, `hist ${i}`);
  }
});

test('KST / Stoch / OBV 的暖机位不画 0，且不越过当前 bar', () => {
  const kst = seriesOf(calc('kst')).map((s) => s.values);
  for (let i = 44; i < 52; i++) assert.equal(kst[1][i], null, `KST signal warmup ${i}`);

  const stoch = seriesOf(calc('stoch')).map((s) => s.values);
  assert.equal(stoch[0][12], null);
  assert.ok(stoch[0][13] !== null);
  assert.equal(stoch[1][14], null);
  assert.ok(stoch[1][15] !== null);

  const obv = seriesOf(calc('obv'))[0].values;
  assert.equal(obv[0], null);
  assert.ok(obv[1] !== null);
});

test('Ichimoku 的转换/基准线不位移，先行 A/B 向右位移 displacement 根', () => {
  const [conversion, base, spanA, spanB] = seriesOf(calc('ichimoku')).map((s) => s.values);
  const displacement = 26;
  for (let i = displacement; i < N; i++) {
    const src = i - displacement;
    if (conversion[src] !== null && base[src] !== null) {
      assert.ok(
        Math.abs(spanA[i] - (conversion[src] + base[src]) / 2) < 1e-9,
        `spanA ${i}`,
      );
    }
  }
  // 先行 B = 52 根 high/low 中点，右移 26 根。
  for (const i of [77, 120, 279, 399]) {
    const src = i - displacement;
    let hi = -Infinity;
    let lo = Infinity;
    for (let j = src - 51; j <= src; j++) {
      hi = Math.max(hi, KLINES[j].high);
      lo = Math.min(lo, KLINES[j].low);
    }
    assert.ok(Math.abs(spanB[i] - (hi + lo) / 2) < 1e-9, `spanB ${i}`);
  }
});

test('振荡指标值域合法', () => {
  const bounded = [
    ['rsi', 0, 100],
    ['stoch', 0, 100],
    ['stochrsi', 0, 100],
    ['williamsr', -100, 0],
    ['adx', 0, 100],
    ['mfi', 0, 100],
  ];
  for (const [id, lo, hi] of bounded) {
    for (const series of seriesOf(calc(id))) {
      for (const v of series.values) {
        if (v === null) continue;
        assert.ok(v >= lo - 1e-9 && v <= hi + 1e-9, `${id}/${series.label}: ${v}`);
      }
    }
  }
});

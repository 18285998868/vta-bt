import {
  createChart,
  createSeriesMarkers,
  ColorType,
  CrosshairMode,
  CandlestickSeries,
  LineSeries,
  HistogramSeries,
  type IPaneApi,
  type Time,
  type ISeriesApi,
  type CandlestickSeriesPartialOptions,
  type CandlestickData,
  type LineData,
  type HistogramData,
  type UTCTimestamp,
  type SeriesMarker,
} from 'lightweight-charts';
import { DrawingManager, type DrawingKind } from 'lightweight-charts-drawing';
import type { Kline, StrategyMarker, StrategyOverlay } from './types';
import type { ActiveIndicator, SubSeries } from './indicators';
import { safeCalc } from './indicators';

const BUFFER = 30;
const INITIAL_VISIBLE_BARS = 50;
const MAIN_STRETCH = 0.6;
const SUB_STRETCH = 0.4;
const DRAWING_HISTORY_LIMIT = 50;
const DRAWING_RECORD_DEBOUNCE_MS = 250;

export type LoadDirection = 'left' | 'right';
export type DrawingTool = DrawingKind;

export interface Viewport {
  /** 毫秒时间戳（含）。 */
  from: number;
  /** 毫秒时间戳（含）。 */
  to: number;
}

export interface ChartManager {
  setData(klines: Kline[]): void;
  /** 回放用：只替换可见前缀，保留指标/策略层并由调用方随后重算。 */
  updateData(klines: Kline[], visibleBars?: number): void;
  prependData(klines: Kline[]): void;
  appendData(klines: Kline[]): void;
  getAllKlines(): Kline[];
  getFirstTime(): number | null;
  getLastTime(): number | null;
  getViewport(): Viewport | null;
  setViewport(v: Viewport | null): void;
  onViewportChange(cb: () => void): () => void;
  applyIndicators(indicators: ActiveIndicator[], klines: Kline[]): void;
  setStrategy(markers: StrategyMarker[], overlays: StrategyOverlay[]): void;
  jumpToTime(time: number): void;
  setDrawingTool(tool: DrawingTool | null): void;
  getDrawingsJSON(): string;
  setDrawingsJSON(json: string): void;
  clearDrawings(): void;
  onDrawingsChange(cb: () => void): () => void;
  undoDrawings(): void;
  redoDrawings(): void;
  destroy(): void;
}

const candleOpts: CandlestickSeriesPartialOptions = {
  upColor: '#a6e3a1',
  downColor: '#f38ba8',
  borderUpColor: '#a6e3a1',
  borderDownColor: '#f38ba8',
  wickUpColor: '#a6e3a1',
  wickDownColor: '#f38ba8',
};

const colorPalette = [
  '#fab387', '#89b4fa', '#a6e3a1', '#f9e2af',
  '#94e2d5', '#eba0ac', '#cba6f7', '#f38ba8',
];

export function createChartManager(
  wrapper: HTMLElement,
  onNearEdge: (dir: LoadDirection) => void,
): ChartManager {
  const chart = createChart(wrapper, {
    layout: {
      background: { type: ColorType.Solid, color: '#1e1e2e' },
      textColor: '#cdd6f4',
    },
    grid: {
      vertLines: { color: '#313244' },
      horzLines: { color: '#313244' },
    },
    crosshair: { mode: CrosshairMode.Normal },
    rightPriceScale: { borderColor: '#45475a' },
    timeScale: { borderColor: '#45475a', timeVisible: false, borderVisible: true },
  });

  const candleSeries = chart.addSeries(CandlestickSeries, candleOpts);
  const seriesMarkers = createSeriesMarkers(candleSeries, []);

  // 绘图层：挂在主图 K 线 series 上；锚点是 {time, price}，与周期无关。
  const drawings = new DrawingManager(chart, candleSeries, { magnet: 'weak' });

  const overlaySeries: ISeriesApi<'Line'>[] = [];
  const overlayKeys: string[] = [];
  // 策略层全量缓存，只渲染已加载窗口（避免污染时间轴逻辑坐标）。
  // 注意：资金曲线不在主图绘制（与价格同轴会压缩K线），绩效只看面板。
  let strategyMarkers: StrategyMarker[] = [];
  let strategyOverlays: StrategyOverlay[] = [];
  const strategyOverlaySeries: ISeriesApi<'Line'>[] = [];
  const strategyOverlayKeys: string[] = [];

  // 子窗格：v5 原生 pane，共享同一条时间轴与十字光标，不再需要多 chart 同步。
  const subPanes: IPaneApi<Time>[] = [];
  type SubSeriesApi = ISeriesApi<'Line'> | ISeriesApi<'Histogram'>;
  interface SubPaneMeta {
    pane: IPaneApi<Time>;
    keys: string[];
    series: SubSeriesApi[];
  }
  const subPaneMeta: SubPaneMeta[] = [];

  let allKlines: Kline[] = [];
  let suppressEdgeCheck = false;

  // ---- viewport ----
  function getViewport(): Viewport | null {
    const range = chart.timeScale().getVisibleRange();
    if (!range) return null;
    return { from: (range.from as number) * 1000, to: (range.to as number) * 1000 };
  }

  function setViewport(v: Viewport | null): void {
    if (!v) {
      chart.timeScale().fitContent();
      return;
    }
    try {
      chart.timeScale().setVisibleRange({
        from: (v.from / 1000) as UTCTimestamp,
        to: (v.to / 1000) as UTCTimestamp,
      });
    } catch {
      // 视口落在当前数据之外（例如换了交易对）：回到最新一段。
      const len = allKlines.length;
      if (len > INITIAL_VISIBLE_BARS) {
        chart.timeScale().setVisibleLogicalRange({ from: len - INITIAL_VISIBLE_BARS, to: len });
      } else {
        chart.timeScale().fitContent();
      }
    }
  }

  // ---- 加载更多 ----
  chart.timeScale().subscribeVisibleLogicalRangeChange((range) => {
    if (!range || suppressEdgeCheck || allKlines.length === 0) return;
    if (range.from < BUFFER) onNearEdge('left');
    if (allKlines.length - range.to < BUFFER) onNearEdge('right');
  });

  // ---- 视口变化（用于会话自动保存）----
  let viewportCb: (() => void) | null = null;
  chart.timeScale().subscribeVisibleTimeRangeChange(() => viewportCb?.());

  // ---- resize ----
  function resize() {
    chart.resize(wrapper.clientWidth, wrapper.clientHeight);
  }

  const ro = new ResizeObserver(() => resize());
  ro.observe(wrapper);

  function toCandle(klines: Kline[]): CandlestickData[] {
    return klines.map((k) => ({
      time: (k.time / 1000) as UTCTimestamp,
      open: k.open,
      high: k.high,
      low: k.low,
      close: k.close,
    }));
  }

  function clearOverlays() {
    for (const s of overlaySeries) {
      chart.removeSeries(s);
    }
    overlaySeries.length = 0;
    overlayKeys.length = 0;
  }

  function clearStrategyOverlays() {
    for (const s of strategyOverlaySeries) {
      chart.removeSeries(s);
    }
    strategyOverlaySeries.length = 0;
    strategyOverlayKeys.length = 0;
  }

  function renderStrategyWindow() {
    if (allKlines.length === 0) {
      seriesMarkers.setMarkers([]);
      clearStrategyOverlays();
      return;
    }
    const from = allKlines[0].time;
    const to = allKlines[allKlines.length - 1].time;
    const inWindow = (t: number) => t >= from && t <= to;
    seriesMarkers.setMarkers(toMarkers(strategyMarkers.filter((m) => inWindow(m.time))));

    const visible = strategyOverlays.map((ov, i) => ({
      key: `${i}:${ov.label}`,
      points: ov.points
        .filter((p) => inWindow(p.time) && p.value !== null)
        .map((p) => ({ time: (p.time / 1000) as UTCTimestamp, value: p.value! })),
    }));
    const sameShape =
      visible.length === strategyOverlaySeries.length &&
      visible.every((def, i) => def.key === strategyOverlayKeys[i]);
    if (sameShape) {
      visible.forEach((def, i) => strategyOverlaySeries[i].setData(def.points));
      return;
    }

    clearStrategyOverlays();
    visible.forEach((def, i) => {
      const s = chart.addSeries(LineSeries, {
        color: colorPalette[i % colorPalette.length],
        lineWidth: 1,
        lastValueVisible: true,
        priceLineVisible: false,
      });
      s.setData(def.points);
      strategyOverlaySeries.push(s);
      strategyOverlayKeys.push(def.key);
    });
  }

  function toMarkers(markers: StrategyMarker[]): SeriesMarker<UTCTimestamp>[] {
    return markers.map((m) => {
      const entry = m.kind === 'entry';
      const long = m.side === 'long';
      return {
        time: (m.time / 1000) as UTCTimestamp,
        position: entry ? (long ? 'belowBar' : 'aboveBar') : long ? 'aboveBar' : 'belowBar',
        color: entry ? (long ? '#a6e3a1' : '#f38ba8') : '#94e2d5',
        shape: entry ? (long ? 'arrowUp' : 'arrowDown') : 'circle',
        text: entry ? (long ? 'L' : 'S') : 'X',
      };
    });
  }

  function clearSubPanes() {
    // 从后往前删，避免索引前移。
    for (let i = chart.panes().length - 1; i >= 1; i--) {
      chart.removePane(i);
    }
    subPanes.length = 0;
    subPaneMeta.length = 0;
    chart.panes()[0]?.setStretchFactor(1);
  }

  function applyPaneStretch() {
    const panes = chart.panes();
    if (panes.length === 0) return;
    const subs = panes.length - 1;
    if (subs === 0) {
      panes[0].setStretchFactor(1);
      return;
    }
    panes[0].setStretchFactor(MAIN_STRETCH);
    for (let i = 1; i < panes.length; i++) {
      panes[i].setStretchFactor(SUB_STRETCH / subs);
    }
  }

  // ---- indicator data helpers: include ALL kline timestamps for visual alignment ----
  function buildLineData(values: (number | null)[], klines: Kline[]): LineData[] {
    const out: LineData[] = [];
    for (let i = 0; i < values.length; i++) {
      if (values[i] !== null) {
        out.push({ time: (klines[i].time / 1000) as UTCTimestamp, value: values[i]! });
      }
    }
    return out;
  }

  function buildHistogramData(values: (number | null)[], klines: Kline[], color: string): HistogramData[] {
    const out: HistogramData[] = [];
    for (let i = 0; i < values.length; i++) {
      if (values[i] !== null) {
        out.push({ time: (klines[i].time / 1000) as UTCTimestamp, value: values[i]!, color });
      }
    }
    return out;
  }

  function addSubSeries(
    pane: IPaneApi<Time>,
    subSeries: SubSeries,
    klines: Kline[],
    color: string,
  ): ISeriesApi<'Line'> | ISeriesApi<'Histogram'> {
    if (subSeries.type === 'histogram') {
      const s = pane.addSeries(HistogramSeries, {
        color: subSeries.color || color,
        priceFormat: { type: 'volume' },
      });
      s.setData(buildHistogramData(subSeries.values, klines, subSeries.color || color));
      return s;
    }
    const s = pane.addSeries(LineSeries, {
      color: subSeries.color || color,
      lineWidth: 1,
    });
    s.setData(buildLineData(subSeries.values, klines));
    return s;
  }

  // ---- 绘图持久化 / 撤销栈 ----
  let drawingHistory: string[] = [drawings.exportJSON()];
  let drawingHistoryIndex = 0;
  let drawingSuppress = false;
  let drawingTimer: number | null = null;
  let drawingsChangeCb: (() => void) | null = null;

  function notifyDrawingsChange() {
    drawingsChangeCb?.();
  }

  function recordDrawings() {
    if (drawingSuppress) return;
    if (drawingTimer !== null) window.clearTimeout(drawingTimer);
    drawingTimer = window.setTimeout(() => {
      drawingTimer = null;
      const json = drawings.exportJSON();
      if (json === drawingHistory[drawingHistoryIndex]) return;
      drawingHistory = drawingHistory.slice(0, drawingHistoryIndex + 1);
      drawingHistory.push(json);
      if (drawingHistory.length > DRAWING_HISTORY_LIMIT) {
        drawingHistory.shift();
      }
      drawingHistoryIndex = drawingHistory.length - 1;
      notifyDrawingsChange();
    }, DRAWING_RECORD_DEBOUNCE_MS);
  }

  const offDrawingChange = drawings.on('change', recordDrawings);

  function restoreDrawingHistory(json: string) {
    drawingSuppress = true;
    try {
      drawings.importJSON(json);
    } finally {
      drawingSuppress = false;
    }
  }

  return {
    setData(klines: Kline[]) {
      clearOverlays();
      clearSubPanes();
      allKlines = klines;
      strategyMarkers = [];
      strategyOverlays = [];
      clearStrategyOverlays();
      candleSeries.setData(toCandle(klines));
      renderStrategyWindow();
      const len = klines.length;
      if (len > INITIAL_VISIBLE_BARS) {
        chart.timeScale().setVisibleLogicalRange({ from: len - INITIAL_VISIBLE_BARS, to: len });
      } else {
        chart.timeScale().fitContent();
      }
    },

    updateData(klines: Kline[], visibleBars = INITIAL_VISIBLE_BARS) {
      allKlines = klines;
      candleSeries.setData(toCandle(klines));
      renderStrategyWindow();
      const len = klines.length;
      suppressEdgeCheck = true;
      if (len > visibleBars) {
        chart.timeScale().setVisibleLogicalRange({ from: len - visibleBars, to: len });
      } else {
        chart.timeScale().fitContent();
      }
      suppressEdgeCheck = false;
    },

    prependData(klines: Kline[]) {
      if (klines.length === 0) return;
      const savedRange = chart.timeScale().getVisibleLogicalRange();
      allKlines = klines.concat(allKlines);
      candleSeries.setData(toCandle(allKlines));
      if (savedRange) {
        const shift = klines.length;
        suppressEdgeCheck = true;
        chart.timeScale().setVisibleLogicalRange({ from: savedRange.from + shift, to: savedRange.to + shift });
        suppressEdgeCheck = false;
      }
      renderStrategyWindow();
    },

    appendData(klines: Kline[]) {
      if (klines.length === 0) return;
      const savedRange = chart.timeScale().getVisibleLogicalRange();
      allKlines = allKlines.concat(klines);
      candleSeries.setData(toCandle(allKlines));
      if (savedRange) {
        suppressEdgeCheck = true;
        chart.timeScale().setVisibleLogicalRange(savedRange);
        suppressEdgeCheck = false;
      }
      renderStrategyWindow();
    },

    getAllKlines() {
      return allKlines;
    },

    getFirstTime() {
      return allKlines.length > 0 ? allKlines[0].time : null;
    },

    getLastTime() {
      return allKlines.length > 0 ? allKlines[allKlines.length - 1].time : null;
    },

    getViewport,

    setViewport,

    onViewportChange(cb: () => void) {
      viewportCb = cb;
      return () => {
        if (viewportCb === cb) viewportCb = null;
      };
    },

    applyIndicators(indicators: ActiveIndicator[], klines: Kline[]) {
      const savedViewport = getViewport();

      allKlines = klines;

      const results = indicators.map((active) => ({
        result: safeCalc(() => active.def.calculate(klines, active.params), active.def.name),
      }));

      interface OverlayDef {
        key: string;
        values: (number | null)[];
        color: string;
      }
      interface SubSeriesDef {
        key: string;
        def: SubSeries;
        color: string;
      }
      interface SubPaneDef {
        key: string;
        series: SubSeriesDef[];
      }
      const overlayDefs: OverlayDef[] = [];
      const subDefs: SubPaneDef[] = [];

      let ci = 0;
      results.forEach(({ result }, ri) => {
        result.overlay?.forEach((ov, oi) => {
          const color = ov.color || colorPalette[ci % colorPalette.length];
          overlayDefs.push({ key: `${ri}:${oi}:${ov.label}:${color}`, values: ov.values, color });
          ci++;
        });
        result.sub?.forEach((paneDef, pi) => {
          const subSeries: SubSeriesDef[] = paneDef.series.map((sub, si) => {
            const color = sub.color || colorPalette[ci % colorPalette.length];
            ci++;
            return { key: `${ri}:${pi}:${si}:${sub.label}:${color}`, def: sub, color };
          });
          subDefs.push({ key: `${ri}:${pi}`, series: subSeries });
        });
      });

      const overlaySame =
        overlaySeries.length === overlayDefs.length &&
        overlayDefs.every((def, i) => def.key === overlayKeys[i]);
      const subSame =
        subPaneMeta.length === subDefs.length &&
        subDefs.every((paneDef, i) => {
          const meta = subPaneMeta[i];
          return (
            meta.keys.length === paneDef.series.length &&
            paneDef.series.every((def, j) => def.key === meta.keys[j])
          );
        });

      let rebuilt = false;
      if (overlaySame) {
        overlayDefs.forEach((def, i) => {
          overlaySeries[i].setData(buildLineData(def.values, klines));
        });
      } else {
        clearOverlays();
        overlayDefs.forEach((def) => {
          const series = chart.addSeries(LineSeries, {
            color: def.color,
            lineWidth: 1,
            lastValueVisible: true,
            priceFormat: { type: 'price' },
          });
          series.setData(buildLineData(def.values, klines));
          overlaySeries.push(series);
          overlayKeys.push(def.key);
        });
        rebuilt = true;
      }

      if (subSame) {
        subDefs.forEach((paneDef, i) => {
          const meta = subPaneMeta[i];
          paneDef.series.forEach((def, j) => {
            const series = meta.series[j];
            if (def.def.type === 'histogram') {
              (series as ISeriesApi<'Histogram'>).setData(
                buildHistogramData(def.def.values, klines, def.color),
              );
            } else {
              (series as ISeriesApi<'Line'>).setData(buildLineData(def.def.values, klines));
            }
          });
        });
      } else {
        clearSubPanes();
        subDefs.forEach((paneDef) => {
          const pane = chart.addPane();
          subPanes.push(pane);
          const meta: SubPaneMeta = { pane, keys: [], series: [] };
          for (const def of paneDef.series) {
            const series = addSubSeries(pane, def.def, klines, def.color);
            meta.keys.push(def.key);
            meta.series.push(series);
          }
          subPaneMeta.push(meta);
        });
        if (subDefs.length > 0) applyPaneStretch();
        rebuilt = true;
      }

      if (rebuilt) resize();
      setViewport(savedViewport);
    },

    setStrategy(markers: StrategyMarker[], overlays: StrategyOverlay[]) {
      strategyMarkers = markers;
      strategyOverlays = overlays;
      renderStrategyWindow();
    },

    jumpToTime(time: number) {
      const idx = allKlines.findIndex((k) => k.time >= time);
      if (idx < 0) return;
      const halfSpan = 30;
      const from = Math.max(0, idx - halfSpan);
      const to = Math.min(allKlines.length, idx + halfSpan);
      chart.timeScale().setVisibleLogicalRange({ from, to });
    },

    setDrawingTool(tool: DrawingTool | null) {
      drawings.setTool(tool);
    },

    getDrawingsJSON() {
      return drawings.exportJSON();
    },

    setDrawingsJSON(json: string) {
      restoreDrawingHistory(json);
      drawingHistory = [drawings.exportJSON()];
      drawingHistoryIndex = 0;
    },

    clearDrawings() {
      drawings.clear();
      const json = drawings.exportJSON();
      if (json !== drawingHistory[drawingHistoryIndex]) {
        drawingHistory = drawingHistory.slice(0, drawingHistoryIndex + 1);
        drawingHistory.push(json);
        if (drawingHistory.length > DRAWING_HISTORY_LIMIT) {
          drawingHistory.shift();
        }
        drawingHistoryIndex = drawingHistory.length - 1;
      }
      notifyDrawingsChange();
    },

    onDrawingsChange(cb: () => void) {
      drawingsChangeCb = cb;
      return () => {
        if (drawingsChangeCb === cb) drawingsChangeCb = null;
      };
    },

    undoDrawings() {
      if (drawingHistoryIndex <= 0) return;
      drawingHistoryIndex--;
      restoreDrawingHistory(drawingHistory[drawingHistoryIndex]);
      notifyDrawingsChange();
    },

    redoDrawings() {
      if (drawingHistoryIndex >= drawingHistory.length - 1) return;
      drawingHistoryIndex++;
      restoreDrawingHistory(drawingHistory[drawingHistoryIndex]);
      notifyDrawingsChange();
    },

    destroy() {
      if (drawingTimer !== null) window.clearTimeout(drawingTimer);
      offDrawingChange();
      drawings.destroy();
      ro.disconnect();
      clearOverlays();
      clearSubPanes();
      chart.remove();
    },
  };
}

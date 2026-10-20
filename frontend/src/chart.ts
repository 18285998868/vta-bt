import {
  createChart,
  ColorType,
  CrosshairMode,
  type IChartApi,
  type ISeriesApi,
  type CandlestickSeriesPartialOptions,
  type CandlestickData,
  type LineData,
  type HistogramData,
  type UTCTimestamp,
  type SeriesMarker,
} from 'lightweight-charts';
import type { Kline, StrategyMarker, StrategyOverlay } from './types';
import type { ActiveIndicator, SubSeries } from './indicators';
import { safeCalc } from './indicators';

const BUFFER = 30;
const INITIAL_VISIBLE_BARS = 50;
const MAIN_HEIGHT_RATIO = 0.6;
const SUB_HEIGHT_RATIO = 0.4;

export type LoadDirection = 'left' | 'right';

export interface ChartManager {
  setData(klines: Kline[]): void;
  prependData(klines: Kline[]): void;
  appendData(klines: Kline[]): void;
  getAllKlines(): Kline[];
  getFirstTime(): number | null;
  getLastTime(): number | null;
  applyIndicators(indicators: ActiveIndicator[], klines: Kline[]): void;
  setStrategy(markers: StrategyMarker[], overlays: StrategyOverlay[]): void;
  jumpToTime(time: number): void;
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
  wrapper.style.display = 'flex';
  wrapper.style.flexDirection = 'column';

  const mainContainer = document.createElement('div');
  mainContainer.style.minHeight = '200px';
  wrapper.appendChild(mainContainer);

  const subContainer = document.createElement('div');
  subContainer.id = 'sub-panes';
  subContainer.style.display = 'none';
  subContainer.style.flexDirection = 'column';
  subContainer.style.minHeight = '0';
  wrapper.appendChild(subContainer);

  function applyHeights() {
    const total = wrapper.clientHeight;
    if (subPanes.length > 0) {
      mainContainer.style.height = `${Math.max(total * MAIN_HEIGHT_RATIO, 200)}px`;
      subContainer.style.display = 'flex';
      subContainer.style.height = `${Math.max(total * SUB_HEIGHT_RATIO, 80)}px`;
    } else {
      mainContainer.style.height = `${Math.max(total, 200)}px`;
      subContainer.style.display = 'none';
      subContainer.style.height = '0';
    }
  }

  const chart = createChart(mainContainer, {
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

  const candleSeries = chart.addCandlestickSeries(candleOpts);
  const overlaySeries: ISeriesApi<'Line'>[] = [];
  // 策略层全量缓存，只渲染已加载窗口（避免污染时间轴逻辑坐标）。
  // 注意：资金曲线不在主图绘制（与价格同轴会压缩K线），绩效只看面板。
  let strategyMarkers: StrategyMarker[] = [];
  let strategyOverlays: StrategyOverlay[] = [];
  const strategyOverlaySeries: ISeriesApi<'Line'>[] = [];
  const subPanes: { chart: IChartApi; container: HTMLElement; series: ISeriesApi<any>[] }[] = [];
  const allCharts: IChartApi[] = [chart];

  let allKlines: Kline[] = [];
  let suppressEdgeCheck = false;

  // ---- time scale sync ----
  let syncing = false;

  function syncRangeToAll(source: IChartApi, range: { from: number; to: number } | null) {
    if (syncing || !range) return;
    syncing = true;
    for (const c of allCharts) {
      if (c !== source) {
        try { c.timeScale().setVisibleLogicalRange(range); } catch (e) {
          console.warn('timeScale sync failed:', e);
        }
      }
    }
    if (allKlines.length > 0 && !suppressEdgeCheck) {
      if (range.from - 0 < BUFFER) onNearEdge('left');
      if (allKlines.length - range.to < BUFFER) onNearEdge('right');
    }
    syncing = false;
  }

  function subscribeTimeScale(ch: IChartApi) {
    ch.timeScale().subscribeVisibleLogicalRangeChange((range) => syncRangeToAll(ch, range));
  }
  subscribeTimeScale(chart);

  // ---- crosshair sync ----
  let crosshairSyncing = false;

  function syncCrosshair(time: UTCTimestamp | undefined, chartIdx: number) {
    if (crosshairSyncing || !time) return;
    crosshairSyncing = true;
    for (let i = 0; i < allCharts.length; i++) {
      if (i === chartIdx) continue;
      const target = allCharts[i];
      if (i === 0) {
        // target is main chart → use candleSeries
        // price=0 is a dummy: we only need the vertical line position
        target.setCrosshairPosition(0, time, candleSeries);
      } else {
        // target is a sub-pane → use its first series
        const pane = subPanes[i - 1];
        if (pane && pane.series.length > 0) {
          target.setCrosshairPosition(0, time, pane.series[0]);
        }
      }
    }
    crosshairSyncing = false;
  }

  function subscribeCrosshair(ch: IChartApi, chartIdx: number) {
    ch.subscribeCrosshairMove((param) => {
      syncCrosshair(param.time as UTCTimestamp | undefined, chartIdx);
    });
  }
  subscribeCrosshair(chart, 0);

  // ---- resize ----
  function resizeAll() {
    applyHeights();
    const w = wrapper.clientWidth;
    chart.resize(w, mainContainer.clientHeight);
    for (const pane of subPanes) {
      pane.chart.resize(pane.container.clientWidth, pane.container.clientHeight);
    }
  }

  const ro = new ResizeObserver(() => resizeAll());
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
  }

  function renderStrategyWindow() {
    for (const s of strategyOverlaySeries) {
      chart.removeSeries(s);
    }
    strategyOverlaySeries.length = 0;
    if (allKlines.length === 0) {
      candleSeries.setMarkers([]);
      return;
    }
    const from = allKlines[0].time;
    const to = allKlines[allKlines.length - 1].time;
    const inWindow = (t: number) => t >= from && t <= to;
    candleSeries.setMarkers(toMarkers(strategyMarkers.filter((m) => inWindow(m.time))));
    strategyOverlays.forEach((ov, i) => {
      const s = chart.addLineSeries({
        color: colorPalette[i % colorPalette.length],
        lineWidth: 1,
        lastValueVisible: true,
        priceLineVisible: false,
      });
      s.setData(
        ov.points
          .filter((p) => inWindow(p.time) && p.value !== null)
          .map((p) => ({ time: (p.time / 1000) as UTCTimestamp, value: p.value! })),
      );
      strategyOverlaySeries.push(s);
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
    for (const pane of subPanes) {
      pane.chart.remove();
      pane.container.remove();
    }
    subPanes.length = 0;
    allCharts.length = 1; // keep main chart
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

  function forceLayout(el: HTMLElement) { void el.offsetHeight; }

  return {
    setData(klines: Kline[]) {
      clearOverlays();
      clearSubPanes();
      allKlines = klines;
      strategyMarkers = [];
      strategyOverlays = [];
      renderStrategyWindow();
      candleSeries.setData(toCandle(klines));
      const len = klines.length;
      if (len > INITIAL_VISIBLE_BARS) {
        chart.timeScale().setVisibleLogicalRange({ from: len - INITIAL_VISIBLE_BARS, to: len });
      } else {
        chart.timeScale().fitContent();
      }
      applyHeights();
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

    applyIndicators(indicators: ActiveIndicator[], klines: Kline[]) {
      const savedRange = chart.timeScale().getVisibleLogicalRange();

      allKlines = klines;
      clearOverlays();
      clearSubPanes();

      const results = indicators.map((active) => ({
        result: safeCalc(() => active.def.calculate(klines, active.params), active.def.name),
      }));
      const hasSub = results.some((r) => r.result.sub && r.result.sub.length > 0);

      let ci = 0;
      for (const { result } of results) {
        if (result.overlay) {
          for (const ov of result.overlay) {
            const series = chart.addLineSeries({
              color: ov.color || colorPalette[ci % colorPalette.length],
              lineWidth: 1,
              lastValueVisible: true,
              priceFormat: { type: 'price' },
            });
            series.setData(buildLineData(ov.values, klines));
            overlaySeries.push(series);
            ci++;
          }
        }
      }

      // create sub-panes (if needed) — make subContainer visible BEFORE chart creation
      if (hasSub) {
        subContainer.style.display = 'flex';
        applyHeights();
        forceLayout(subContainer);

        for (const { result } of results) {
          if (result.sub) {
            for (const paneDef of result.sub) {
              const paneContainer = document.createElement('div');
              paneContainer.style.flex = '1';
              paneContainer.style.minHeight = '60px';
              paneContainer.style.borderTop = '1px solid #313244';
              subContainer.appendChild(paneContainer);

              forceLayout(paneContainer);

              const paneChart = createChart(paneContainer, {
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
                timeScale: {
                  borderColor: '#45475a',
                  timeVisible: false,
                  visible: false,
                },
              });

              const paneSeries: ISeriesApi<any>[] = [];
              for (const sub of paneDef.series) {
                const s = addSubSeries(paneChart, sub, klines, colorPalette[ci % colorPalette.length]);
                paneSeries.push(s);
                ci++;
              }

              // add invisible sentinel series covering ALL klines so logical-to-time mapping matches main chart
              const sentinelData: LineData[] = klines.map((k) => ({
                time: (k.time / 1000) as UTCTimestamp,
                value: 0,
              }));
              paneChart.addLineSeries({
                color: 'rgba(0,0,0,0)',
                lineWidth: 1,
                priceFormat: { type: 'price' },
                lastValueVisible: false,
                autoscaleInfoProvider: () => null,
              }).setData(sentinelData);

              const chartIdx = allCharts.length;
              subscribeTimeScale(paneChart);
              subscribeCrosshair(paneChart, chartIdx);
              allCharts.push(paneChart);
              subPanes.push({ chart: paneChart, container: paneContainer, series: paneSeries });
            }
          }
        }
      }

      applyHeights();
      resizeAll();

      // sync time scale to savedRange after everything is set up
      if (savedRange) {
        for (const c of allCharts) {
          try { c.timeScale().setVisibleLogicalRange(savedRange); } catch (_) { /* */ }
        }
      }
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
      syncRangeToAll(chart, { from, to });
    },

    destroy() {
      ro.disconnect();
      clearOverlays();
      clearSubPanes();
      chart.remove();
      mainContainer.remove();
    },
  };

  function addSubSeries(
    paneChart: IChartApi,
    subSeries: SubSeries,
    klines: Kline[],
    color: string,
  ): ISeriesApi<any> {
    if (subSeries.type === 'histogram') {
      const s = paneChart.addHistogramSeries({
        color: subSeries.color || color,
        priceFormat: { type: 'volume' },
      });
      s.setData(buildHistogramData(subSeries.values, klines, subSeries.color || color));
      return s;
    }
    const s = paneChart.addLineSeries({
      color: subSeries.color || color,
      lineWidth: 1,
    });
    s.setData(buildLineData(subSeries.values, klines));
    return s;
  }
}

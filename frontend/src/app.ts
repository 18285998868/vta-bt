import {
  createLayout,
  deleteLayout,
  fetchDrawings,
  fetchKlines,
  fetchLayout,
  fetchLayouts,
  fetchMarketTypes,
  fetchPairs,
  fetchPreferences,
  fetchStrategies,
  fetchStrategy,
  fetchTimeframes,
  putPreferences,
  replaceDrawings,
  updateLayout,
  type DrawingDocument,
  type LayoutInput,
} from './api';
import {
  createChartManager,
  type ChartManager,
  type DrawingTool,
  type LoadDirection,
  type Viewport,
} from './chart';
import { findIndicator, getAllIndicators, type ActiveIndicator, type IndicatorDef } from './indicators';
import type { Kline, StrategyResult } from './types';

const PAGE_SIZE = 200;
const REPLAY_LIMIT = 2000;
const REPLAY_STRATEGY_DEBOUNCE_MS = 300;
const PAIR_DROPDOWN_LIMIT = 100;
const APP_VERSION = 'p6';
const SESSION_SAVE_DEBOUNCE_MS = 800;
const DRAWING_SAVE_DEBOUNCE_MS = 600;

function defaultTomlFor(strategyType: string): string {
  if (strategyType === 'bbrk') return 'type = "bbrk"\nsigma = 2.0';
  return `type = "${strategyType}"\nperiod = 20`;
}

interface IndicatorState {
  id: string;
  params: Record<string, number>;
}

interface StrategyState {
  toml: string;
  applied: boolean;
}

/** 回放只维护游标；完整 K 线仍在 allKlines，图表只拿 [0..index] 前缀。 */
interface ReplayState {
  startIndex: number;
  index: number;
  playing: boolean;
  speed: number;
  timer: number | null;
}

/** 可持久化的会话快照（last_session / 命名布局共用）。 */
interface SessionState {
  market_type: string;
  pair: string;
  timeframe: string;
  indicators: IndicatorState[];
  strategy: StrategyState | null;
  viewport: Viewport | null;
}

function isSessionState(value: unknown): value is SessionState {
  if (!value || typeof value !== 'object') return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v.market_type === 'string' &&
    typeof v.pair === 'string' &&
    typeof v.timeframe === 'string' &&
    Array.isArray(v.indicators)
  );
}

export class App {
  private chart: ChartManager | null = null;
  private loading = false;
  private allKlines: Kline[] = [];
  private replay: ReplayState | null = null;
  private preReplayViewport: Viewport | null = null;
  private activeIndicators: ActiveIndicator[] = [];
  private allPairs: string[] = [];
  private activeStrategyToml: string | null = null;
  private marketSelect: HTMLSelectElement;
  private pairSearch: HTMLInputElement;
  private pairDropdown: HTMLElement;
  private timeframeSelect: HTMLSelectElement;
  private strategySelect: HTMLSelectElement;
  private strategyToml: HTMLTextAreaElement;
  private strategyEditor: HTMLElement;
  private strategyError: HTMLElement;
  private strategyPanel: HTMLElement;
  private strategyPanelName: HTMLElement;
  private strategyPanelBody: HTMLElement;
  private infoEl: HTMLElement;
  private chartContainer: HTMLElement;
  private indicatorsEl: HTMLElement;
  private layoutSelect: HTMLSelectElement;
  private drawingToolSelect: HTMLSelectElement;
  private replayEnterBtn: HTMLButtonElement;
  private replayControls: HTMLElement;
  private replayPlayBtn: HTMLButtonElement;
  private replayStepBtn: HTMLButtonElement;
  private replayResetBtn: HTMLButtonElement;
  private replayExitBtn: HTMLButtonElement;
  private replaySpeedSelect: HTMLSelectElement;
  private replayPositionEl: HTMLElement;
  private _pair = '';
  private _timeframe = '';
  /** 恢复/加载布局期间抑制自动保存。 */
  private suppressSave = false;
  /** 初始化完成前不保存（避免把半初始化状态写成 last_session）。 */
  private ready = false;
  private saveTimer: number | null = null;
  private drawingSaveTimer: number | null = null;
  /** 当前 chart 的绘图已从服务端加载完成；未完成前禁止回写，避免用空画布覆盖。 */
  private drawingsLoaded = false;
  private drawingLoadToken = 0;
  private unbindViewport: (() => void) | null = null;
  private unbindDrawings: (() => void) | null = null;
  /** 策略请求序号：只接受最后一次请求，回放高频更新时丢弃过期结果。 */
  private strategySeq = 0;
  private strategyTimer: number | null = null;
  private strategyInFlight = false;
  private strategyPending = false;

  constructor(container: HTMLElement) {
    container.innerHTML = `
      <div class="toolbar">
        <label>市场</label>
        <select id="market-select"></select>
        <label>交易对</label>
        <div class="pair-search-wrap">
          <input id="pair-search" type="text" placeholder="搜索交易对..." autocomplete="off" disabled />
          <div id="pair-dropdown" class="pair-dropdown"></div>
        </div>
        <label>周期</label>
        <select id="timeframe-select" disabled><option>—</option></select>
        <label>布局</label>
        <select id="layout-select"><option value="">— 布局 —</option></select>
        <button id="layout-save" class="time-jump-btn">保存</button>
        <button id="layout-delete" class="time-jump-btn">删除</button>
        <input type="date" id="time-jump" class="time-jump" />
        <button id="time-jump-btn" class="time-jump-btn">跳转</button>
        <div class="replay-controls" id="replay-controls">
          <label>回放</label>
          <button id="replay-play" class="time-jump-btn">▶ 播放</button>
          <button id="replay-step" class="time-jump-btn">步进</button>
          <select id="replay-speed" title="回放速度">
            <option value="0.5">0.5×</option>
            <option value="1" selected>1×</option>
            <option value="2">2×</option>
            <option value="4">4×</option>
            <option value="8">8×</option>
          </select>
          <button id="replay-reset" class="time-jump-btn">重置</button>
          <button id="replay-exit" class="time-jump-btn">退出</button>
          <span id="replay-position" class="replay-position"></span>
        </div>
        <button id="replay-enter" class="time-jump-btn">回放</button>
        <label>策略</label>
        <select id="strategy-select"></select>
        <button id="strategy-config" class="time-jump-btn">⚙配置</button>
        <button id="strategy-apply" class="time-jump-btn">应用</button>
        <button id="strategy-clear" class="time-jump-btn">清除</button>
        <div id="strategy-editor" class="strategy-editor">
          <div class="strategy-editor-title">策略 TOML（与 vta 离线配置同口径）</div>
          <textarea id="strategy-toml" class="strategy-toml" rows="8" spellcheck="false"></textarea>
          <div class="strategy-editor-actions">
            <button id="strategy-editor-apply" class="time-jump-btn">应用并关闭</button>
            <button id="strategy-editor-close" class="time-jump-btn">关闭</button>
          </div>
          <div id="strategy-error" class="strategy-error"></div>
        </div>
        <div id="strategy-panel" class="strategy-panel">
          <div class="strategy-panel-title">
            <span id="strategy-panel-name"></span>
            <button id="strategy-panel-close" class="param-close">✕</button>
          </div>
          <div id="strategy-panel-body"></div>
        </div>
        <div class="indicator-search-wrap">
          <input id="indicator-search" type="text" placeholder="搜索指标..." autocomplete="off" />
          <div id="indicator-dropdown" class="indicator-dropdown"></div>
        </div>
        <div id="active-indicators" class="active-indicators"></div>
        <label>绘图</label>
        <select id="drawing-tool">
          <option value="">光标</option>
          <option value="trend-line">趋势线</option>
          <option value="horizontal-line">水平线</option>
          <option value="vertical-line">垂直线</option>
          <option value="rectangle">矩形</option>
          <option value="fib-retracement">斐波那契</option>
          <option value="text">文字</option>
          <option value="arrow">箭头</option>
        </select>
        <button id="drawing-undo" class="time-jump-btn">撤销</button>
        <button id="drawing-redo" class="time-jump-btn">重做</button>
        <button id="drawing-clear" class="time-jump-btn">清空</button>
        <span class="info" id="info"></span>
        <span class="app-version" id="app-version"></span>
      </div>
      <div class="chart-container" id="chart-container">
        <div class="loading">选择交易对和周期</div>
      </div>
    `;

    this.marketSelect = container.querySelector('#market-select')!;
    this.pairSearch = container.querySelector('#pair-search')!;
    this.pairDropdown = container.querySelector('#pair-dropdown')!;
    this.timeframeSelect = container.querySelector('#timeframe-select')!;
    this.strategySelect = container.querySelector('#strategy-select')!;
    this.strategyToml = container.querySelector('#strategy-toml')!;
    this.strategyEditor = container.querySelector('#strategy-editor')!;
    this.strategyError = container.querySelector('#strategy-error')!;
    this.strategyPanel = container.querySelector('#strategy-panel')!;
    this.strategyPanelName = container.querySelector('#strategy-panel-name')!;
    this.strategyPanelBody = container.querySelector('#strategy-panel-body')!;
    this.infoEl = container.querySelector('#info')!;
    this.chartContainer = container.querySelector('#chart-container')!;
    this.indicatorsEl = container.querySelector('#active-indicators') as HTMLElement;
    this.layoutSelect = container.querySelector('#layout-select') as HTMLSelectElement;
    this.drawingToolSelect = container.querySelector('#drawing-tool') as HTMLSelectElement;
    this.replayEnterBtn = container.querySelector('#replay-enter') as HTMLButtonElement;
    this.replayControls = container.querySelector('#replay-controls') as HTMLElement;
    this.replayPlayBtn = container.querySelector('#replay-play') as HTMLButtonElement;
    this.replayStepBtn = container.querySelector('#replay-step') as HTMLButtonElement;
    this.replayResetBtn = container.querySelector('#replay-reset') as HTMLButtonElement;
    this.replayExitBtn = container.querySelector('#replay-exit') as HTMLButtonElement;
    this.replaySpeedSelect = container.querySelector('#replay-speed') as HTMLSelectElement;
    this.replayPositionEl = container.querySelector('#replay-position') as HTMLElement;

    const searchInput = container.querySelector('#indicator-search') as HTMLInputElement;
    const dropdown = container.querySelector('#indicator-dropdown') as HTMLElement;
    const timeJump = container.querySelector('#time-jump') as HTMLInputElement;
    const timeJumpBtn = container.querySelector('#time-jump-btn') as HTMLButtonElement;
    const applyBtn = container.querySelector('#strategy-apply') as HTMLButtonElement;
    const clearBtn = container.querySelector('#strategy-clear') as HTMLButtonElement;
    const configBtn = container.querySelector('#strategy-config') as HTMLButtonElement;
    const editorApplyBtn = container.querySelector('#strategy-editor-apply') as HTMLButtonElement;
    const editorCloseBtn = container.querySelector('#strategy-editor-close') as HTMLButtonElement;
    const panelCloseBtn = container.querySelector('#strategy-panel-close') as HTMLButtonElement;
    const layoutSaveBtn = container.querySelector('#layout-save') as HTMLButtonElement;
    const layoutDeleteBtn = container.querySelector('#layout-delete') as HTMLButtonElement;
    const drawingUndoBtn = container.querySelector('#drawing-undo') as HTMLButtonElement;
    const drawingRedoBtn = container.querySelector('#drawing-redo') as HTMLButtonElement;
    const drawingClearBtn = container.querySelector('#drawing-clear') as HTMLButtonElement;

    (container.querySelector('#app-version') as HTMLElement).textContent = APP_VERSION;

    this.marketSelect.addEventListener('change', () => this.onMarketChange());
    this.timeframeSelect.addEventListener('change', () => this.onTimeframeChange());
    this.strategySelect.addEventListener('change', () => {
      this.strategyToml.value = defaultTomlFor(this.strategySelect.value);
    });
    applyBtn.addEventListener('click', () => this.applyStrategy());
    clearBtn.addEventListener('click', () => this.clearStrategy());
    configBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      this.strategyEditor.classList.toggle('visible');
    });
    editorApplyBtn.addEventListener('click', () => {
      this.strategyEditor.classList.remove('visible');
      void this.applyStrategy();
    });
    editorCloseBtn.addEventListener('click', () => this.strategyEditor.classList.remove('visible'));
    panelCloseBtn.addEventListener('click', () => this.clearStrategy());

    this.layoutSelect.addEventListener('change', () => {
      const id = this.layoutSelect.value;
      if (id) void this.loadLayout(id);
    });
    layoutSaveBtn.addEventListener('click', () => void this.saveLayout());
    layoutDeleteBtn.addEventListener('click', () => void this.deleteSelectedLayout());

    this.drawingToolSelect.addEventListener('change', () => {
      const tool = this.drawingToolSelect.value;
      this.chart?.setDrawingTool(tool ? (tool as DrawingTool) : null);
    });
    drawingUndoBtn.addEventListener('click', () => this.chart?.undoDrawings());
    drawingRedoBtn.addEventListener('click', () => this.chart?.redoDrawings());
    drawingClearBtn.addEventListener('click', () => {
      if (!this.chart) return;
      if (!confirm('清空当前交易对的所有绘图？')) return;
      this.chart.clearDrawings();
    });

    const onJump = () => this.onTimeJump(timeJump);
    timeJump.addEventListener('change', onJump);
    timeJumpBtn.addEventListener('click', onJump);

    this.replayEnterBtn.addEventListener('click', () => void this.enterReplay());
    this.replayPlayBtn.addEventListener('click', () => this.toggleReplay());
    this.replayStepBtn.addEventListener('click', () => this.stepReplay());
    this.replayResetBtn.addEventListener('click', () => this.resetReplay());
    this.replayExitBtn.addEventListener('click', () => this.exitReplay());
    this.replaySpeedSelect.addEventListener('change', () => {
      this.setReplaySpeed(parseFloat(this.replaySpeedSelect.value));
    });

    this.pairSearch.addEventListener('input', () => this.onPairSearchInput());
    this.pairSearch.addEventListener('focus', () => this.onPairSearchInput());
    this.pairSearch.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        const first = this.matchPairs(this.pairSearch.value)[0];
        if (first) this.selectPair(first);
      } else if (e.key === 'Escape') {
        this.pairDropdown.classList.remove('visible');
      }
    });

    searchInput.addEventListener('input', () => this.onSearchInput(searchInput, dropdown));
    searchInput.addEventListener('focus', () => this.onSearchInput(searchInput, dropdown));
    document.addEventListener('click', (e) => {
      const t = e.target as HTMLElement;
      if (!t.closest('.indicator-search-wrap')) {
        dropdown.classList.remove('visible');
      }
      if (!t.closest('.pair-search-wrap')) {
        this.pairDropdown.classList.remove('visible');
      }
      if (!t.closest('#strategy-editor') && !t.closest('#strategy-config') && !t.closest('#strategy-select')) {
        this.strategyEditor.classList.remove('visible');
      }
    });

    this.init();
  }

  private async init(): Promise<void> {
    try {
      const types = await fetchMarketTypes();
      for (const t of types) {
        const opt = document.createElement('option');
        opt.value = t;
        opt.textContent = t;
        this.marketSelect.appendChild(opt);
      }
      await this.loadStrategyTypes();
      await this.refreshLayoutList();

      const preferences: Record<string, unknown> = await fetchPreferences().catch(() => ({}));
      const lastSession = preferences['last_session'];
      if (isSessionState(lastSession) && types.includes(lastSession.market_type)) {
        await this.applySession(lastSession);
      } else if (types.length > 0) {
        this.marketSelect.value = types[0];
        await this.loadPairs();
      }
    } catch (e) {
      this.infoEl.textContent = `加载失败: ${e}`;
    } finally {
      this.ready = true;
      this.scheduleSave();
    }
  }

  // ---- 会话 / 命名布局 ----

  private collectSessionState(): SessionState {
    return {
      market_type: this.marketSelect.value,
      pair: this._pair,
      timeframe: this._timeframe,
      indicators: this.activeIndicators.map((a) => ({
        id: a.def.id,
        params: { ...a.params },
      })),
      strategy: this.strategyToml.value.trim()
        ? { toml: this.strategyToml.value, applied: this.activeStrategyToml !== null }
        : null,
      viewport: this.chart?.getViewport() ?? null,
    };
  }

  private scheduleSave(): void {
    if (this.suppressSave || !this.ready) return;
    if (this.saveTimer !== null) window.clearTimeout(this.saveTimer);
    this.saveTimer = window.setTimeout(() => {
      this.saveTimer = null;
      void this.saveSessionNow();
    }, SESSION_SAVE_DEBOUNCE_MS);
  }

  private async saveSessionNow(): Promise<void> {
    if (this.suppressSave || !this.ready || !this._pair || !this._timeframe) return;
    try {
      await putPreferences({ last_session: this.collectSessionState() });
    } catch (e) {
      console.warn('保存会话失败', e);
    }
  }

  private async flushPendingSaves(): Promise<void> {
    if (this.saveTimer !== null) {
      window.clearTimeout(this.saveTimer);
      this.saveTimer = null;
      await this.saveSessionNow();
    }
    await this.flushDrawings();
  }

  // ---- 回放 ----

  private activeKlines(): Kline[] {
    if (!this.replay) return this.allKlines;
    return this.allKlines.slice(0, this.replay.index + 1);
  }

  private dataInfoText(): string {
    if (!this.replay) return `${this.allKlines.length} 条K线`;
    const r = this.replay;
    const bar = this.allKlines[r.index];
    const dt = bar
      ? new Date(bar.time).toLocaleString('zh-CN', { hour12: false })
      : '';
    const suffix = r.index >= this.allKlines.length - 1 ? ' · 已到末尾' : '';
    return `回放 ${r.index + 1}/${this.allKlines.length}${dt ? ` · ${dt}` : ''}${suffix}`;
  }

  private updateReplayUi(): void {
    const active = this.replay !== null;
    this.replayControls.classList.toggle('visible', active);
    this.replayEnterBtn.disabled = active;
    if (!this.replay) {
      this.replayPositionEl.textContent = '';
      this.replayPlayBtn.textContent = '▶ 播放';
      return;
    }
    const r = this.replay;
    const atEnd = r.index >= this.allKlines.length - 1;
    this.replayPlayBtn.textContent = r.playing ? '⏸ 暂停' : '▶ 播放';
    this.replayPlayBtn.disabled = atEnd;
    this.replayStepBtn.disabled = atEnd;
    this.replaySpeedSelect.value = String(r.speed);
    this.replayPositionEl.textContent = `${r.index + 1} / ${this.allKlines.length}`;
  }

  private async enterReplay(): Promise<void> {
    if (this.replay || this.loading) return;
    if (!this.chart || !this._pair || !this._timeframe || this.allKlines.length === 0) {
      this.infoEl.textContent = '请先加载K线数据';
      return;
    }
    const chart = this.chart;
    const market = this.marketSelect.value;
    const pair = this._pair;
    const timeframe = this._timeframe;
    this.loading = true;
    this.infoEl.textContent = '加载回放数据...';
    try {
      const anchor = chart.getLastTime();
      const data = await fetchKlines({
        marketType: market,
        pair,
        timeframe,
        limit: REPLAY_LIMIT,
        ...(anchor !== null ? { before: anchor + 1 } : {}),
      });
      if (this.chart !== chart || this.marketSelect.value !== market || this._pair !== pair || this._timeframe !== timeframe) {
        return;
      }
      const full = data.klines;
      if (full.length < 2) {
        this.infoEl.textContent = '回放数据不足';
        return;
      }
      const viewport = chart.getViewport();
      let startIndex = 0;
      if (viewport) {
        const idx = full.findIndex((k) => k.time >= viewport.from);
        if (idx >= 0) startIndex = idx;
      }
      startIndex = Math.min(startIndex, full.length - 1);

      this.preReplayViewport = viewport;
      this.allKlines = full;
      this.replay = {
        startIndex,
        index: startIndex,
        playing: false,
        speed: 1,
        timer: null,
      };
      chart.setData(this.activeKlines());
      this.refreshIndicators();
      this.updateReplayUi();
      this.infoEl.textContent = this.dataInfoText();
      this.scheduleReplayStrategy(0);
      this.scheduleSave();
    } catch (e) {
      this.infoEl.textContent = `加载回放失败: ${e}`;
    } finally {
      this.loading = false;
    }
  }

  private toggleReplay(): void {
    if (!this.replay) return;
    if (this.replay.playing) this.pauseReplay();
    else this.playReplay();
  }

  private playReplay(): void {
    const r = this.replay;
    if (!r || r.playing) return;
    if (r.index >= this.allKlines.length - 1) {
      this.infoEl.textContent = '回放已到末尾';
      return;
    }
    r.playing = true;
    this.startReplayTimer();
    this.updateReplayUi();
    this.infoEl.textContent = this.dataInfoText();
  }

  private pauseReplay(): void {
    const r = this.replay;
    if (!r) return;
    r.playing = false;
    this.stopReplayTimer();
    this.updateReplayUi();
    this.infoEl.textContent = this.dataInfoText();
    this.scheduleReplayStrategy(0);
  }

  private stepReplay(): void {
    const r = this.replay;
    if (!r) return;
    if (r.playing) this.pauseReplay();
    if (r.index >= this.allKlines.length - 1) {
      this.infoEl.textContent = this.dataInfoText();
      return;
    }
    r.index++;
    this.renderReplayPrefix();
    this.scheduleReplayStrategy(0);
  }

  private resetReplay(): void {
    const r = this.replay;
    if (!r) return;
    r.playing = false;
    this.stopReplayTimer();
    r.index = r.startIndex;
    this.renderReplayPrefix();
    this.scheduleReplayStrategy(0);
  }

  private exitReplay(): void {
    if (!this.replay) return;
    const viewport = this.preReplayViewport;
    this.stopReplayTimer();
    if (this.strategyTimer !== null) {
      window.clearTimeout(this.strategyTimer);
      this.strategyTimer = null;
    }
    this.strategySeq++;
    this.strategyInFlight = false;
    this.strategyPending = false;
    this.replay = null;
    this.preReplayViewport = null;
    this.chart?.setData(this.allKlines);
    this.refreshIndicators();
    if (viewport) this.chart?.setViewport(viewport);
    this.updateReplayUi();
    this.infoEl.textContent = this.dataInfoText();
    if (this.activeStrategyToml) void this.refreshStrategy();
    this.scheduleSave();
  }

  private setReplaySpeed(speed: number): void {
    const r = this.replay;
    if (!r || !Number.isFinite(speed) || speed <= 0) return;
    r.speed = speed;
    if (r.playing) this.startReplayTimer();
  }

  private startReplayTimer(): void {
    this.stopReplayTimer();
    const r = this.replay;
    if (!r) return;
    const interval = Math.max(50, Math.round(1000 / r.speed));
    r.timer = window.setInterval(() => this.tickReplay(), interval);
  }

  private stopReplayTimer(): void {
    const r = this.replay;
    if (r && r.timer !== null) {
      window.clearInterval(r.timer);
      r.timer = null;
    }
  }

  private tickReplay(): void {
    const r = this.replay;
    if (!r || !r.playing) return;
    if (r.index >= this.allKlines.length - 1) {
      this.pauseReplay();
      return;
    }
    r.index++;
    this.renderReplayPrefix();
    if (r.index >= this.allKlines.length - 1) this.pauseReplay();
  }

  private renderReplayPrefix(): void {
    const r = this.replay;
    const chart = this.chart;
    if (!r || !chart) return;
    chart.updateData(this.activeKlines());
    this.refreshIndicators();
    this.updateReplayUi();
    this.infoEl.textContent = this.dataInfoText();
    this.scheduleReplayStrategy();
  }

  private replayBefore(): number | undefined {
    const r = this.replay;
    if (!r) return undefined;
    const next = this.allKlines[r.index + 1];
    if (next) return next.time;
    return this.allKlines[r.index].time + 1;
  }

  private scheduleReplayStrategy(delay = REPLAY_STRATEGY_DEBOUNCE_MS): void {
    if (!this.replay || !this.activeStrategyToml) return;
    if (this.strategyInFlight) {
      this.strategyPending = true;
      return;
    }
    if (this.strategyTimer !== null) {
      if (delay > 0) return;
      window.clearTimeout(this.strategyTimer);
      this.strategyTimer = null;
    }
    this.strategyTimer = window.setTimeout(() => {
      this.strategyTimer = null;
      void this.refreshStrategy();
    }, delay);
  }

  private disposeReplay(): void {
    this.stopReplayTimer();
    if (this.strategyTimer !== null) {
      window.clearTimeout(this.strategyTimer);
      this.strategyTimer = null;
    }
    this.strategySeq++;
    this.strategyInFlight = false;
    this.strategyPending = false;
    this.replay = null;
    this.preReplayViewport = null;
    this.updateReplayUi();
  }

  private async applySession(state: SessionState): Promise<void> {
    this.suppressSave = true;
    try {
      this.teardownChart('加载中...');
      this.marketSelect.value = state.market_type;
      this.clearStrategyState();
      this.activeIndicators = state.indicators.flatMap((s) => {
        const def = findIndicator(s.id);
        return def ? [{ def, params: { ...def.defaultParams, ...s.params } }] : [];
      });
      if (state.strategy?.toml) {
        this.strategyToml.value = state.strategy.toml;
        const type = state.strategy.toml.match(/^\s*type\s*=\s*"([^"]+)"/m)?.[1];
        if (type && [...this.strategySelect.options].some((o) => o.value === type)) {
          this.strategySelect.value = type;
        }
      }
      this.activeStrategyToml = state.strategy?.applied ? state.strategy.toml : null;

      await this.loadPairs(state.pair, state.timeframe);
      if (this.chart && state.viewport) this.chart.setViewport(state.viewport);
      if (this.activeStrategyToml) await this.refreshStrategy();
    } finally {
      this.suppressSave = false;
    }
  }

  private async refreshLayoutList(selectedId?: string): Promise<void> {
    try {
      const layouts = await fetchLayouts();
      const current = selectedId ?? this.layoutSelect.value;
      this.layoutSelect.innerHTML = '<option value="">— 布局 —</option>';
      for (const layout of layouts) {
        const opt = document.createElement('option');
        opt.value = layout.id;
        opt.textContent = `${layout.name} · ${layout.pair} ${layout.timeframe}`;
        this.layoutSelect.appendChild(opt);
      }
      if (current && layouts.some((l) => l.id === current)) {
        this.layoutSelect.value = current;
      }
    } catch (e) {
      console.warn('加载布局列表失败', e);
    }
  }

  private async loadLayout(id: string): Promise<void> {
    try {
      await this.flushPendingSaves();
      const layout = await fetchLayout(id);
      if (!isSessionState(layout.payload)) {
        throw new Error('布局数据格式不兼容');
      }
      await this.applySession(layout.payload);
      this.layoutSelect.value = id;
      this.infoEl.textContent = `已加载布局: ${layout.name}`;
      this.scheduleSave();
    } catch (e) {
      this.infoEl.textContent = `加载布局失败: ${e}`;
    }
  }

  private async saveLayout(): Promise<void> {
    if (!this._pair || !this._timeframe) {
      this.infoEl.textContent = '请先加载K线数据';
      return;
    }
    const selected = this.layoutSelect.selectedOptions[0];
    const selectedName = selected?.value
      ? selected.textContent?.split(' · ')[0] ?? ''
      : '';
    const name = prompt('布局名称', selectedName || `${this._pair} ${this._timeframe}`);
    if (name === null) return;
    const trimmed = name.trim();
    if (!trimmed) return;
    await this.flushDrawings();

    const state = this.collectSessionState();
    const input: LayoutInput = {
      name: trimmed,
      market_type: state.market_type,
      pair: state.pair,
      timeframe: state.timeframe,
      payload: state,
    };
    try {
      const id = this.layoutSelect.value;
      const saved = id ? await updateLayout(id, input) : await createLayout(input);
      await this.refreshLayoutList(saved.id);
      this.infoEl.textContent = `布局已保存: ${saved.name}`;
    } catch (e) {
      this.infoEl.textContent = `保存布局失败: ${e}`;
    }
  }

  private async deleteSelectedLayout(): Promise<void> {
    const id = this.layoutSelect.value;
    if (!id) {
      this.infoEl.textContent = '请先选择一个布局';
      return;
    }
    const name = this.layoutSelect.selectedOptions[0]?.textContent ?? id;
    if (!confirm(`删除布局「${name}」？`)) return;
    try {
      await deleteLayout(id);
      await this.refreshLayoutList();
      this.infoEl.textContent = '布局已删除';
    } catch (e) {
      this.infoEl.textContent = `删除布局失败: ${e}`;
    }
  }

  // ---- 绘图持久化 ----

  private bindChart(): void {
    const chart = this.chart;
    if (!chart) return;
    this.drawingsLoaded = false;
    this.unbindViewport = chart.onViewportChange(() => this.scheduleSave());
    this.unbindDrawings = chart.onDrawingsChange(() => this.scheduleDrawingSave());
    const tool = this.drawingToolSelect.value;
    chart.setDrawingTool(tool ? (tool as DrawingTool) : null);
    void this.loadDrawingsForPair();
  }

  private async loadDrawingsForPair(): Promise<void> {
    const chart = this.chart;
    if (!chart || !this._pair) return;
    const market = this.marketSelect.value;
    const pair = this._pair;
    const token = ++this.drawingLoadToken;
    try {
      const drawings = await fetchDrawings(market, pair);
      if (token !== this.drawingLoadToken || this.chart !== chart) return;
      if (this.marketSelect.value !== market || this._pair !== pair) return;
      chart.setDrawingsJSON(JSON.stringify(drawings));
      this.drawingsLoaded = true;
    } catch (e) {
      console.warn('加载绘图失败', e);
    }
  }

  private scheduleDrawingSave(): void {
    if (this.suppressSave || !this.ready || !this.chart || !this._pair || !this.drawingsLoaded) return;
    if (this.drawingSaveTimer !== null) window.clearTimeout(this.drawingSaveTimer);
    this.drawingSaveTimer = window.setTimeout(() => {
      this.drawingSaveTimer = null;
      void this.saveDrawingsNow();
    }, DRAWING_SAVE_DEBOUNCE_MS);
  }

  private async saveDrawingsNow(): Promise<void> {
    if (!this.chart || !this._pair || !this.drawingsLoaded) return;
    let drawings: DrawingDocument;
    try {
      const parsed = JSON.parse(this.chart.getDrawingsJSON()) as unknown;
      if (!Array.isArray(parsed)) return;
      drawings = parsed as DrawingDocument;
    } catch {
      return;
    }
    try {
      await replaceDrawings(this.marketSelect.value, this._pair, drawings);
    } catch (e) {
      console.warn('保存绘图失败', e);
    }
  }

  private async flushDrawings(): Promise<void> {
    if (this.drawingSaveTimer !== null) {
      window.clearTimeout(this.drawingSaveTimer);
      this.drawingSaveTimer = null;
    }
    if (this.drawingsLoaded) await this.saveDrawingsNow();
  }

  private teardownChart(placeholder: string): void {
    this.disposeReplay();
    this.drawingLoadToken += 1;
    this.drawingsLoaded = false;
    if (this.saveTimer !== null) {
      window.clearTimeout(this.saveTimer);
      this.saveTimer = null;
    }
    if (this.drawingSaveTimer !== null) {
      window.clearTimeout(this.drawingSaveTimer);
      this.drawingSaveTimer = null;
    }
    this.unbindViewport?.();
    this.unbindViewport = null;
    this.unbindDrawings?.();
    this.unbindDrawings = null;
    this.chart?.destroy();
    this.chart = null;
    this.allKlines = [];
    this.chartContainer.innerHTML = `<div class="loading">${placeholder}</div>`;
    this.indicatorsEl.innerHTML = '';
    document.querySelectorAll('.param-editor').forEach((e) => e.remove());
  }

  private async loadStrategyTypes(): Promise<void> {
    let types: string[];
    try {
      types = await fetchStrategies();
    } catch {
      types = ['bbrk', 'sma', 'ema'];
    }
    this.strategySelect.innerHTML = '';
    for (const t of types) {
      const opt = document.createElement('option');
      opt.value = t;
      opt.textContent = t;
      this.strategySelect.appendChild(opt);
    }
    this.strategySelect.value = types.includes('bbrk') ? 'bbrk' : types[0];
    this.strategyToml.value = defaultTomlFor(this.strategySelect.value);
  }

  private async loadPairs(preferredPair?: string, preferredTimeframe?: string): Promise<void> {
    const pairs = await fetchPairs(this.marketSelect.value);
    this.allPairs = pairs;
    this.pairSearch.value = '';
    this.pairSearch.placeholder = pairs.length > 0 ? `从 ${pairs.length} 个交易对搜索...` : '无交易对';
    this.pairSearch.disabled = pairs.length === 0;
    if (pairs.length > 0) {
      const pair = preferredPair && pairs.includes(preferredPair) ? preferredPair : pairs[0];
      this._pair = pair;
      this.pairSearch.value = pair;
      await this.loadTimeframes(preferredTimeframe);
    } else {
      this._pair = '';
      this._timeframe = '';
    }
  }

  private matchPairs(q: string): string[] {
    const query = q.trim().toUpperCase();
    const matched = query
      ? this.allPairs.filter((p) => p.includes(query))
      : this.allPairs.slice();
    matched.sort((a, b) => {
      const as = a.startsWith(query) ? 0 : 1;
      const bs = b.startsWith(query) ? 0 : 1;
      return as - bs || (a < b ? -1 : 1);
    });
    return matched.slice(0, PAIR_DROPDOWN_LIMIT);
  }

  private onPairSearchInput() {
    const matched = this.matchPairs(this.pairSearch.value);
    this.pairDropdown.innerHTML = '';
    if (matched.length === 0) {
      this.pairDropdown.classList.remove('visible');
      return;
    }
    for (const p of matched) {
      const item = document.createElement('div');
      item.className = 'pair-dropdown-item';
      if (p === this._pair) item.classList.add('selected');
      item.textContent = p;
      item.addEventListener('click', () => this.selectPair(p));
      this.pairDropdown.appendChild(item);
    }
    this.pairDropdown.classList.add('visible');
  }

  private async selectPair(pair: string): Promise<void> {
    this.pairDropdown.classList.remove('visible');
    if (pair === this._pair && this._timeframe) return;
    await this.flushPendingSaves();
    this._pair = pair;
    this.pairSearch.value = pair;
    this._timeframe = '';
    this.timeframeSelect.disabled = true;
    this.activeIndicators = [];
    this.clearStrategyState();
    this.teardownChart('选择周期');
    await this.loadTimeframes();
    this.scheduleSave();
  }

  private async loadTimeframes(preferredTimeframe?: string): Promise<void> {
    const timeframes = await fetchTimeframes(this.marketSelect.value, this._pair);
    this.timeframeSelect.innerHTML = '';
    const placeholder = document.createElement('option');
    placeholder.value = '';
    placeholder.textContent = '选择周期';
    placeholder.disabled = true;
    placeholder.selected = true;
    this.timeframeSelect.appendChild(placeholder);
    for (const tf of timeframes) {
      const opt = document.createElement('option');
      opt.value = tf;
      opt.textContent = tf;
      this.timeframeSelect.appendChild(opt);
    }
    this.timeframeSelect.disabled = timeframes.length === 0;
    if (timeframes.length > 0) {
      const tf = preferredTimeframe && timeframes.includes(preferredTimeframe)
        ? preferredTimeframe
        : timeframes[0];
      this.timeframeSelect.value = tf;
      this._timeframe = tf;
      await this.loadKlines();
    } else {
      this._timeframe = '';
    }
  }

  private async loadKlines(dir?: LoadDirection): Promise<void> {
    if (this.loading || this.replay) return;
    this.loading = true;
    this.infoEl.textContent = '加载中...';
    try {
      let klines;
      if (dir === 'left') {
        const before = this.chart!.getFirstTime()!;
        klines = await fetchKlines({ marketType: this.marketSelect.value, pair: this._pair, timeframe: this._timeframe, limit: PAGE_SIZE, before });
        if (klines.klines.length > 0) {
          this.allKlines = klines.klines.concat(this.allKlines);
          this.chart!.prependData(klines.klines);
          this.refreshIndicators();
        }
      } else if (dir === 'right') {
        const after = this.chart!.getLastTime()!;
        klines = await fetchKlines({ marketType: this.marketSelect.value, pair: this._pair, timeframe: this._timeframe, limit: PAGE_SIZE, after });
        if (klines.klines.length > 0) {
          this.allKlines = this.allKlines.concat(klines.klines);
          this.chart!.appendData(klines.klines);
          this.refreshIndicators();
        }
      } else {
        this.chartContainer.innerHTML = '<div class="loading">加载中...</div>';
        klines = await fetchKlines({ marketType: this.marketSelect.value, pair: this._pair, timeframe: this._timeframe, limit: PAGE_SIZE });
        this.allKlines = klines.klines;
        this.chartContainer.innerHTML = '<div id="chart" style="height:100%;width:100%"></div>';
        const chartDiv = this.chartContainer.querySelector('#chart')!;
        this.chart = createChartManager(chartDiv as HTMLElement, (d) => this.loadKlines(d));
        this.bindChart();
        this.chart.setData(klines.klines);
        this.refreshIndicators();
      }
      this.infoEl.textContent = this.dataInfoText();
      if (!dir) this.scheduleSave();
    } catch (e) {
      if (!dir) this.chartContainer.innerHTML = `<div class="loading">加载失败: ${e}</div>`;
      this.infoEl.textContent = '加载失败';
    } finally {
      this.loading = false;
    }
  }

  private refreshIndicators() {
    this.renderActiveIndicators();
    if (this.chart) {
      const klines = this.activeKlines();
      if (klines.length > 0) this.chart.applyIndicators(this.activeIndicators, klines);
    }
    this.scheduleSave();
  }

  private async applyStrategy(): Promise<void> {
    if (!this.chart || !this._pair || !this._timeframe) {
      this.infoEl.textContent = '请先加载K线数据';
      return;
    }
    const toml = this.strategyToml.value.trim();
    if (!toml) {
      this.infoEl.textContent = '策略 TOML 为空';
      return;
    }
    this.activeStrategyToml = toml;
    await this.refreshStrategy();
    this.scheduleSave();
  }

  private async refreshStrategy(): Promise<void> {
    const toml = this.activeStrategyToml;
    const chart = this.chart;
    if (!toml || !chart || this.allKlines.length === 0) return;
    const market = this.marketSelect.value;
    const pair = this._pair;
    const timeframe = this._timeframe;
    const before = this.replayBefore();
    const seq = ++this.strategySeq;
    this.strategyInFlight = true;
    this.strategyError.textContent = '';
    this.infoEl.textContent = this.replay
      ? `${this.dataInfoText()} · 策略计算中...`
      : '策略计算中...';
    try {
      const r = await fetchStrategy({
        marketType: market,
        pair,
        timeframe,
        strategyToml: toml,
        ...(before !== undefined ? { before } : {}),
      });
      if (
        seq !== this.strategySeq ||
        this.chart !== chart ||
        this.marketSelect.value !== market ||
        this._pair !== pair ||
        this._timeframe !== timeframe ||
        this.activeStrategyToml !== toml
      ) {
        return;
      }
      chart.setStrategy(r.markers, r.overlays);
      this.renderStrategyPanel(r);
    } catch (e) {
      if (seq === this.strategySeq) {
        const msg = `策略失败: ${e}`;
        this.strategyError.textContent = msg;
        this.infoEl.textContent = msg;
      }
    } finally {
      if (seq === this.strategySeq) {
        this.strategyInFlight = false;
        if (this.strategyPending) {
          this.strategyPending = false;
          this.scheduleReplayStrategy();
        }
      }
    }
  }

  private renderStrategyPanel(r: StrategyResult) {
    const s = r.summary;
    // null = 该指标算不出来（NaN/Inf），与"真的是 0"区分开，显示为 n/a。
    const num = (v: number | null, d: number): string => (v === null ? 'n/a' : v.toFixed(d));
    const pct = (v: number | null, d = 1): string =>
      v === null ? 'n/a' : `${v >= 0 ? '+' : ''}${v.toFixed(d)}%`;
    const rows: [string, string][] = [
      ['交易笔数', `${s.num_trades}`],
      ['胜率', `${num(s.win_rate, 1)}%`],
      ['总收益', pct(s.total_return_pct)],
      ['最大回撤', `${num(s.max_drawdown_pct, 1)}%`],
      ['Sharpe', num(s.sharpe_ratio, 2)],
      ['Sortino', num(s.sortino_ratio, 2)],
      ['盈亏比', num(s.profit_factor, 2)],
      ['期望', s.expectancy === null ? 'n/a' : `${(s.expectancy * 100).toFixed(3)}%`],
      ['期末权益', num(s.final_equity, 2)],
      ['年换手', num(s.turnover_per_year, 1)],
      ['固定腿收益', pct(s.fixed_total_return)],
      ['固定腿Sharpe', num(s.fixed_sharpe_ratio, 2)],
      ['固定腿回撤', `${num(s.fixed_max_drawdown_pct, 1)}%`],
      ['固定腿单笔期望', `${num(s.fixed_expectancy_bps, 1)}bp`],
      ['固定腿盈亏比', num(s.fixed_profit_factor, 2)],
    ];
    // 参数网格：请求几档 / 实跑几档 / 哪些被跳过。
    const g = r.param_grid;
    if (g && g.requested > 1) {
      rows.push(['参数档位', `${g.ran}/${g.requested} 档（${g.params.join(', ')}）`]);
      for (const sk of g.skipped ?? []) {
        rows.push([`跳过 ${sk.params}`, sk.reason]);
      }
    }
    this.strategyPanelName.textContent = `策略绩效 · ${r.strategy} · ${r.count}根`;
    this.strategyPanelBody.innerHTML = rows
      .map(([k, v]) => `<div class="strategy-row"><span>${k}</span><span>${v}</span></div>`)
      .join('');
    this.strategyPanel.classList.add('visible');
    this.infoEl.textContent = this.dataInfoText();
  }

  private clearStrategy() {
    this.strategySeq++;
    if (this.strategyTimer !== null) {
      window.clearTimeout(this.strategyTimer);
      this.strategyTimer = null;
    }
    this.strategyInFlight = false;
    this.strategyPending = false;
    this.clearStrategyState();
    this.chart?.setStrategy([], []);
    this.scheduleSave();
  }

  private clearStrategyState() {
    this.activeStrategyToml = null;
    this.strategyError.textContent = '';
    this.strategyPanel.classList.remove('visible');
  }

  private onSearchInput(input: HTMLInputElement, dropdown: HTMLElement) {
    const q = input.value.trim().toLowerCase();
    const all = getAllIndicators();
    const matched = q ? all.filter((i) => i.name.toLowerCase().includes(q) || i.description.toLowerCase().includes(q)) : all;
    dropdown.innerHTML = '';
    if (matched.length === 0) { dropdown.classList.remove('visible'); return; }
    for (const ind of matched) {
      const item = document.createElement('div');
      item.className = 'indicator-dropdown-item';
      item.innerHTML = `
        <span class="indicator-name">${ind.name}</span>
        <span class="indicator-cat ${ind.category}">${ind.category === 'main' ? '主图' : '副图'}</span>
        <span class="indicator-desc">${ind.description}</span>
      `;
      item.addEventListener('click', () => this.addIndicator(ind));
      dropdown.appendChild(item);
    }
    dropdown.classList.add('visible');
  }

  private addIndicator(def: IndicatorDef) {
    if (this.activeIndicators.some((a) => a.def.id === def.id)) return;
    this.activeIndicators.push({ def, params: { ...def.defaultParams } });
    (document.querySelector('#indicator-search') as HTMLInputElement).value = '';
    document.querySelector('#indicator-dropdown')?.classList.remove('visible');
    this.refreshIndicators();
  }

  private removeIndicator(id: string) {
    this.activeIndicators = this.activeIndicators.filter((a) => a.def.id !== id);
    this.refreshIndicators();
  }

  private editIndicator(id: string) {
    const active = this.activeIndicators.find((a) => a.def.id === id);
    if (!active || active.def.paramDefs.length === 0) return;

    const existing = document.querySelector(`.param-editor[data-id="${id}"]`);
    if (existing) { existing.remove(); return; }

    const editor = document.createElement('div');
    editor.className = 'param-editor';
    editor.dataset.id = id;
    editor.innerHTML = `<div class="param-editor-title">${active.def.name} 参数</div>`;

    for (const pd of active.def.paramDefs) {
      const row = document.createElement('div');
      row.className = 'param-row';
      const inp = document.createElement('input');
      inp.type = 'number';
      inp.value = String(active.params[pd.key]);
      if (pd.min !== undefined) inp.min = String(pd.min);
      if (pd.max !== undefined) inp.max = String(pd.max);
      inp.step = String(pd.step ?? 1);
      inp.dataset.key = pd.key;
      row.innerHTML = `<label>${pd.name}</label>`;
      row.appendChild(inp);
      inp.addEventListener('change', () => {
        active.params[pd.key] = parseFloat(inp.value) || pd.default;
        this.refreshIndicators();
      });
      editor.appendChild(row);
    }

    const closeBtn = document.createElement('button');
    closeBtn.className = 'param-close';
    closeBtn.textContent = '✕';
    closeBtn.addEventListener('click', () => editor.remove());
    editor.querySelector('.param-editor-title')!.after(closeBtn);

    document.body.appendChild(editor);
    const chip = document.querySelector(`[data-indicator-id="${id}"]`) as HTMLElement;
    if (chip) {
      const rect = chip.getBoundingClientRect();
      editor.style.top = `${rect.bottom + 4}px`;
      editor.style.left = `${rect.left}px`;
    }
  }

  private renderActiveIndicators() {
    this.indicatorsEl.innerHTML = '';
    for (const active of this.activeIndicators) {
      const chip = document.createElement('span');
      chip.className = `indicator-chip ${active.def.category}`;
      chip.dataset.indicatorId = active.def.id;
      chip.innerHTML = `
        <span class="chip-label">${active.def.name}</span>
        ${active.def.paramDefs.length > 0 ? '<span class="chip-edit">⚙</span>' : ''}
        <span class="chip-remove">✕</span>
      `;
      const editBtn = chip.querySelector('.chip-edit');
      if (editBtn) editBtn.addEventListener('click', (e) => { e.stopPropagation(); this.editIndicator(active.def.id); });
      chip.querySelector('.chip-remove')!.addEventListener('click', (e) => { e.stopPropagation(); this.removeIndicator(active.def.id); });
      this.indicatorsEl.appendChild(chip);
    }
  }

  private async onMarketChange(): Promise<void> {
    await this.flushPendingSaves();
    this._pair = '';
    this._timeframe = '';
    this.allPairs = [];
    this.pairSearch.value = '';
    this.pairSearch.placeholder = '搜索交易对...';
    this.pairSearch.disabled = true;
    this.pairDropdown.classList.remove('visible');
    this.timeframeSelect.disabled = true;
    this.activeIndicators = [];
    this.clearStrategyState();
    this.teardownChart('选择交易对和周期');
    await this.loadPairs();
    this.scheduleSave();
  }

  private async onTimeframeChange(): Promise<void> {
    await this.flushPendingSaves();
    this._timeframe = this.timeframeSelect.value;
    this.activeIndicators = [];
    this.clearStrategyState();
    this.teardownChart('加载中...');
    if (this._timeframe) await this.loadKlines();
    this.scheduleSave();
  }

  private onTimeJump(input: HTMLInputElement) {
    if (!this.chart) {
      this.infoEl.textContent = '请先加载数据';
      return;
    }
    if (!input.value) return;
    const ms = new Date(input.value).getTime();
    if (isNaN(ms)) {
      this.infoEl.textContent = '无效日期格式';
      return;
    }
    this.chart.jumpToTime(ms);
  }
}

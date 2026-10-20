import { fetchMarketTypes, fetchPairs, fetchTimeframes, fetchKlines, fetchStrategy, fetchStrategies } from './api';
import { createChartManager, type ChartManager, type LoadDirection } from './chart';
import { getAllIndicators, type ActiveIndicator, type IndicatorDef } from './indicators';
import type { StrategyResult } from './types';

const PAGE_SIZE = 200;
const PAIR_DROPDOWN_LIMIT = 100;
const APP_VERSION = 'p4';

function defaultTomlFor(strategyType: string): string {
  if (strategyType === 'bbrk') return 'type = "bbrk"\nsigma = 2.0';
  return `type = "${strategyType}"\nperiod = 20`;
}

export class App {
  private chart: ChartManager | null = null;
  private loading = false;
  private strategyLoading = false;
  private allKlines: import('./types').Kline[] = [];
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
  private _pair = '';
  private _timeframe = '';

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
        <input type="date" id="time-jump" class="time-jump" />
        <button id="time-jump-btn" class="time-jump-btn">跳转</button>
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

    const onJump = () => this.onTimeJump(timeJump);
    timeJump.addEventListener('change', onJump);
    timeJumpBtn.addEventListener('click', onJump);

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
      if (types.length > 0) {
        this.marketSelect.value = types[0];
        await this.loadPairs();
      }
    } catch (e) {
      this.infoEl.textContent = `加载失败: ${e}`;
    }
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

  private async loadPairs(): Promise<void> {
    const pairs = await fetchPairs(this.marketSelect.value);
    this.allPairs = pairs;
    this.pairSearch.value = '';
    this.pairSearch.placeholder = pairs.length > 0 ? `从 ${pairs.length} 个交易对搜索...` : '无交易对';
    this.pairSearch.disabled = pairs.length === 0;
    if (pairs.length > 0) {
      this._pair = pairs[0];
      this.pairSearch.value = pairs[0];
      await this.loadTimeframes();
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
    this._pair = pair;
    this.pairSearch.value = pair;
    this._timeframe = '';
    this.timeframeSelect.disabled = true;
    this.chart?.destroy();
    this.chart = null;
    this.allKlines = [];
    this.activeIndicators = [];
    this.clearStrategyState();
    this.chartContainer.innerHTML = '<div class="loading">选择周期</div>';
    this.indicatorsEl.innerHTML = '';
    document.querySelectorAll('.param-editor').forEach((e) => e.remove());
    await this.loadTimeframes();
  }

  private async loadTimeframes(): Promise<void> {
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
    this.timeframeSelect.disabled = false;
    if (timeframes.length > 0) {
      this.timeframeSelect.value = timeframes[0];
      this._timeframe = timeframes[0];
      await this.loadKlines();
    }
  }

  private async loadKlines(dir?: LoadDirection): Promise<void> {
    if (this.loading) return;
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
        this.chart.setData(klines.klines);
        this.clearStrategyState();
        this.refreshIndicators();
      }
      this.infoEl.textContent = `${this.allKlines.length} 条K线`;
    } catch (e) {
      if (!dir) this.chartContainer.innerHTML = `<div class="loading">加载失败: ${e}</div>`;
      this.infoEl.textContent = '加载失败';
    } finally {
      this.loading = false;
    }
  }

  private refreshIndicators() {
    this.renderActiveIndicators();
    if (this.chart && this.allKlines.length > 0) {
      this.chart.applyIndicators(this.activeIndicators, this.allKlines);
    }
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
  }

  private async refreshStrategy(): Promise<void> {
    if (!this.activeStrategyToml || !this.chart || this.allKlines.length === 0) return;
    if (this.strategyLoading) return;
    this.strategyLoading = true;
    this.strategyError.textContent = '';
    this.infoEl.textContent = '策略计算中...';
    try {
      const r = await fetchStrategy({
        marketType: this.marketSelect.value,
        pair: this._pair,
        timeframe: this._timeframe,
        strategyToml: this.activeStrategyToml,
      });
      this.chart.setStrategy(r.markers, r.overlays);
      this.renderStrategyPanel(r);
    } catch (e) {
      const msg = `策略失败: ${e}`;
      this.strategyError.textContent = msg;
      this.infoEl.textContent = msg;
    } finally {
      this.strategyLoading = false;
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
    this.infoEl.textContent = `${this.allKlines.length} 条K线`;
  }

  private clearStrategy() {
    this.clearStrategyState();
    this.chart?.setStrategy([], []);
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
    this._pair = '';
    this._timeframe = '';
    this.allPairs = [];
    this.pairSearch.value = '';
    this.pairSearch.placeholder = '搜索交易对...';
    this.pairSearch.disabled = true;
    this.pairDropdown.classList.remove('visible');
    this.timeframeSelect.disabled = true;
    this.chart?.destroy();
    this.chart = null;
    this.allKlines = [];
    this.activeIndicators = [];
    this.clearStrategyState();
    this.chartContainer.innerHTML = '<div class="loading">选择交易对和周期</div>';
    this.indicatorsEl.innerHTML = '';
    document.querySelectorAll('.param-editor').forEach((e) => e.remove());
    await this.loadPairs();
  }

  private async onTimeframeChange(): Promise<void> {
    this._timeframe = this.timeframeSelect.value;
    this.chart?.destroy();
    this.chart = null;
    this.allKlines = [];
    this.activeIndicators = [];
    this.clearStrategyState();
    this.indicatorsEl.innerHTML = '';
    document.querySelectorAll('.param-editor').forEach((e) => e.remove());
    if (this._timeframe) await this.loadKlines();
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

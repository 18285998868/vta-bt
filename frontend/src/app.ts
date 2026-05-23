import { fetchMarketTypes, fetchPairs, fetchTimeframes, fetchKlines } from './api';
import { createChartManager, type ChartManager, type LoadDirection } from './chart';
import { getAllIndicators, type ActiveIndicator, type IndicatorDef } from './indicators';

const PAGE_SIZE = 200;

export class App {
  private chart: ChartManager | null = null;
  private loading = false;
  private allKlines: import('./types').Kline[] = [];
  private activeIndicators: ActiveIndicator[] = [];
  private marketSelect: HTMLSelectElement;
  private pairSelect: HTMLSelectElement;
  private timeframeSelect: HTMLSelectElement;
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
        <select id="pair-select" disabled><option>—</option></select>
        <label>周期</label>
        <select id="timeframe-select" disabled><option>—</option></select>
        <input type="date" id="time-jump" class="time-jump" />
        <button id="time-jump-btn" class="time-jump-btn">跳转</button>
        <div class="indicator-search-wrap">
          <input id="indicator-search" type="text" placeholder="搜索指标..." autocomplete="off" />
          <div id="indicator-dropdown" class="indicator-dropdown"></div>
        </div>
        <div id="active-indicators" class="active-indicators"></div>
        <span class="info" id="info"></span>
      </div>
      <div class="chart-container" id="chart-container">
        <div class="loading">选择交易对和周期</div>
      </div>
    `;

    this.marketSelect = container.querySelector('#market-select')!;
    this.pairSelect = container.querySelector('#pair-select')!;
    this.timeframeSelect = container.querySelector('#timeframe-select')!;
    this.infoEl = container.querySelector('#info')!;
    this.chartContainer = container.querySelector('#chart-container')!;
    this.indicatorsEl = container.querySelector('#active-indicators') as HTMLElement;

    const searchInput = container.querySelector('#indicator-search') as HTMLInputElement;
    const dropdown = container.querySelector('#indicator-dropdown') as HTMLElement;
    const timeJump = container.querySelector('#time-jump') as HTMLInputElement;
    const timeJumpBtn = container.querySelector('#time-jump-btn') as HTMLButtonElement;

    this.marketSelect.addEventListener('change', () => this.onMarketChange());
    this.pairSelect.addEventListener('change', () => this.onPairChange());
    this.timeframeSelect.addEventListener('change', () => this.onTimeframeChange());

    const onJump = () => this.onTimeJump(timeJump);
    timeJump.addEventListener('change', onJump);
    timeJumpBtn.addEventListener('click', onJump);

    searchInput.addEventListener('input', () => this.onSearchInput(searchInput, dropdown));
    searchInput.addEventListener('focus', () => this.onSearchInput(searchInput, dropdown));
    document.addEventListener('click', (e) => {
      if (!(e.target as HTMLElement).closest('.indicator-search-wrap')) {
        dropdown.classList.remove('visible');
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
      if (types.length > 0) {
        this.marketSelect.value = types[0];
        await this.loadPairs();
      }
    } catch (e) {
      this.infoEl.textContent = `加载失败: ${e}`;
    }
  }

  private async loadPairs(): Promise<void> {
    const pairs = await fetchPairs(this.marketSelect.value);
    this.pairSelect.innerHTML = '';
    const placeholder = document.createElement('option');
    placeholder.value = '';
    placeholder.textContent = '选择交易对';
    placeholder.disabled = true;
    placeholder.selected = true;
    this.pairSelect.appendChild(placeholder);
    for (const p of pairs) {
      const opt = document.createElement('option');
      opt.value = p;
      opt.textContent = p;
      this.pairSelect.appendChild(opt);
    }
    this.pairSelect.disabled = false;
    if (pairs.length > 0) {
      this.pairSelect.value = pairs[0];
      this._pair = pairs[0];
      await this.loadTimeframes();
    }
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
    if (this.activeIndicators.length > 0 && this.chart && this.allKlines.length > 0) {
      this.chart.applyIndicators(this.activeIndicators, this.allKlines);
    }
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
    this.pairSelect.disabled = true;
    this.timeframeSelect.disabled = true;
    this.chart?.destroy();
    this.chart = null;
    this.allKlines = [];
    this.activeIndicators = [];
    this.chartContainer.innerHTML = '<div class="loading">选择交易对和周期</div>';
    this.indicatorsEl.innerHTML = '';
    document.querySelectorAll('.param-editor').forEach((e) => e.remove());
    await this.loadPairs();
  }

  private async onPairChange(): Promise<void> {
    this._pair = this.pairSelect.value;
    this._timeframe = '';
    this.timeframeSelect.disabled = true;
    this.chart?.destroy();
    this.chart = null;
    this.allKlines = [];
    this.activeIndicators = [];
    this.chartContainer.innerHTML = '<div class="loading">选择周期</div>';
    this.indicatorsEl.innerHTML = '';
    document.querySelectorAll('.param-editor').forEach((e) => e.remove());
    if (this._pair) await this.loadTimeframes();
  }

  private async onTimeframeChange(): Promise<void> {
    this._timeframe = this.timeframeSelect.value;
    this.chart?.destroy();
    this.chart = null;
    this.allKlines = [];
    this.activeIndicators = [];
    this.indicatorsEl.innerHTML = '';
    document.querySelectorAll('.param-editor').forEach((e) => e.remove());
    if (this._timeframe) await this.loadKlines();
  }

  private onTimeJump(input: HTMLInputElement) {
    if (!this.chart || !input.value) return;
    const ms = new Date(input.value).getTime();
    if (isNaN(ms)) return;
    this.chart.jumpToTime(ms);
  }
}

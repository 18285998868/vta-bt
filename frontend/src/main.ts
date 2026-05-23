import './style.css';
import { App } from './app';

const appEl = document.getElementById('app');
if (!appEl) throw new Error('#app not found');

new App(appEl);

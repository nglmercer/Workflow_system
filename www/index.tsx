import { h, render } from 'preact';
import { App } from './components/App.tsx';

const root = document.getElementById('app');
if (root) {
  render(h(App, null), root);
}

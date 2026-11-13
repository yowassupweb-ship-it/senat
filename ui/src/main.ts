import '@fontsource-variable/inter';
import './theme.css';
import './app.css';
import './views.css';
import { mount } from 'svelte';
import { restoreSizes } from './lib/layout';

// the sizes the person dragged the areas to, in place before the first paint
restoreSizes();
import App from './App.svelte';

mount(App, { target: document.getElementById('app')! });

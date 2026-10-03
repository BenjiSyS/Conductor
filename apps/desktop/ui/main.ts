import './styles/tokens.css';
import './styles/base.css';
import { mount } from 'svelte';
import App from './components/App.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('missing #app');
mount(App, { target });

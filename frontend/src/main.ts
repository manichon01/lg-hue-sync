import { mount } from 'svelte';
import App from './App.svelte';
import { isTv } from './lib/tv';
import './app.css';
import './tv.css';
if (isTv) document.documentElement.classList.add('tv');
mount(App, { target: document.getElementById('app')! });

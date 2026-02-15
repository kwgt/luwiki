import { createApp } from 'vue';
import App from './App.vue';
import './styles/main.css';
import './styles/markdown-themes.css';
import './styles/prism-themes.css';
import 'katex/dist/katex.min.css';

createApp(App).mount('#wiki-root');

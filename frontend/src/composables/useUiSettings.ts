import { computed, onMounted, ref, watch } from 'vue';
import { getMetaContent } from '../lib/pageCommon';

type FontFamilyMap = {
  ui: string;
  sans: string;
  serif: string;
  mono: string;
  code: string;
};

type EditorKeymap = 'default' | 'vim' | 'emacs' | 'vscode';
type DiffMode = 'lines' | 'words' | 'chars' | 'patch';

const THEME_OPTIONS = ['light', 'dark'];
const FONT_OPTIONS = [
  { value: 'sans', label: 'Sans' },
  { value: 'serif', label: 'Serif' },
  { value: 'mono', label: 'Mono' },
];

const EDITOR_KEYMAP_OPTIONS = [
  { value: 'default', label: 'Default' },
  { value: 'vim', label: 'Vim' },
  { value: 'emacs', label: 'Emacs' },
  { value: 'vscode', label: 'VSCode' },
];
const DIFF_MODE_OPTIONS = [
  { value: 'lines', label: '行' },
  { value: 'words', label: '単語' },
  { value: 'chars', label: '文字' },
  { value: 'patch', label: 'パッチ' },
];

function applyTheme(theme: string): void {
  document.documentElement.setAttribute('data-theme', theme);
  document.body.setAttribute('data-theme', theme);
  localStorage.setItem('luwiki-theme', theme);
}

function applyUiFont(fontFamily: string): void {
  document.documentElement.style.setProperty('--ui-font-family', fontFamily);
}

function applyFontSettings(font: string, size: number): void {
  localStorage.setItem('luwiki-md-font', font);
  localStorage.setItem('luwiki-md-font-size', size.toString());
}

function applyCodeFontSettings(size: number): void {
  localStorage.setItem('luwiki-md-code-font-size', size.toString());
}

function resolveFontFamily(value: string, map: FontFamilyMap): string {
  if (value === 'serif') {
    return map.serif;
  }
  if (value === 'mono') {
    return map.mono;
  }
  return map.sans;
}

function loadFontFamilyMap(): FontFamilyMap {
  const sans = getMetaContent('frontend-md-font-sans') ?? 'sans-serif';
  const serif = getMetaContent('frontend-md-font-serif') ?? 'serif';
  const mono = getMetaContent('frontend-md-font-mono') ?? 'monospace';
  const code = getMetaContent('frontend-md-code-font') ?? mono;
  const ui = getMetaContent('frontend-ui-font') ?? 'sans-serif';

  return { ui, sans, serif, mono, code };
}

export function useUiSettings() {
  const selectedTheme = ref(THEME_OPTIONS[0]);
  const selectedFont = ref(FONT_OPTIONS[0].value);
  const selectedFontSize = ref(15);
  const selectedCodeFontSize = ref(15);
  const selectedEditorKeymap = ref<EditorKeymap>('default');
  const selectedEditorLineNumbers = ref(true);
  const selectedDiffMode = ref<DiffMode>('lines');
  const fontFamilyMap = ref<FontFamilyMap>({
    ui: 'sans-serif',
    sans: 'sans-serif',
    serif: 'serif',
    mono: 'monospace',
    code: 'monospace',
  });

  const markdownThemeClass = computed(() =>
    selectedTheme.value === 'dark'
      ? 'markdown-theme-github-dark'
      : 'markdown-theme-github',
  );
  const prismThemeClass = computed(() =>
    selectedTheme.value === 'dark' ? 'prism-theme-dark' : 'prism-theme-light',
  );
  const codeFontFamily = computed(() => fontFamilyMap.value.code);
  const editorStyle = computed(() => ({
    '--cm-font-family': codeFontFamily.value,
    '--cm-font-size': `${selectedCodeFontSize.value}px`,
  }));

  const markdownStyle = computed(() => ({
    '--md-font-family': resolveFontFamily(selectedFont.value, fontFamilyMap.value),
    '--md-code-font-family': fontFamilyMap.value.code,
    '--md-font-size': `${selectedFontSize.value}px`,
    '--md-code-font-size': `${selectedCodeFontSize.value}px`,
  }));

  onMounted(() => {
    fontFamilyMap.value = loadFontFamilyMap();
    applyUiFont(fontFamilyMap.value.ui);

    const savedTheme = localStorage.getItem('luwiki-theme');
    if (savedTheme && THEME_OPTIONS.includes(savedTheme)) {
      selectedTheme.value = savedTheme;
    }
    const savedFont = localStorage.getItem('luwiki-md-font');
    if (savedFont && FONT_OPTIONS.some((font) => font.value === savedFont)) {
      selectedFont.value = savedFont;
    }
    const savedFontSize = localStorage.getItem('luwiki-md-font-size');
    if (savedFontSize) {
      const parsed = Number(savedFontSize);
      if (!Number.isNaN(parsed) && parsed >= 12 && parsed <= 22) {
        selectedFontSize.value = parsed;
      }
    }
    const savedCodeFontSize = localStorage.getItem('luwiki-md-code-font-size');
    if (savedCodeFontSize) {
      const parsed = Number(savedCodeFontSize);
      if (!Number.isNaN(parsed) && parsed >= 12 && parsed <= 22) {
        selectedCodeFontSize.value = parsed;
      }
    } else {
      selectedCodeFontSize.value = selectedFontSize.value;
    }
    const savedKeymap = localStorage.getItem('luwiki-editor-keymap');
    if (savedKeymap && EDITOR_KEYMAP_OPTIONS.some((option) => option.value === savedKeymap)) {
      selectedEditorKeymap.value = savedKeymap as EditorKeymap;
    }
    const savedLineNumbers = localStorage.getItem('luwiki-editor-line-numbers');
    if (savedLineNumbers !== null) {
      selectedEditorLineNumbers.value = savedLineNumbers === '1';
    }
    const savedDiffMode = localStorage.getItem('luwiki-diff-mode');
    if (savedDiffMode && DIFF_MODE_OPTIONS.some((option) => option.value === savedDiffMode)) {
      selectedDiffMode.value = savedDiffMode as DiffMode;
    }
    applyTheme(selectedTheme.value);
    applyFontSettings(selectedFont.value, selectedFontSize.value);
    applyCodeFontSettings(selectedCodeFontSize.value);
  });

  watch(selectedTheme, (theme) => {
    applyTheme(theme);
  });

  watch([selectedFont, selectedFontSize], ([font, size]) => {
    applyFontSettings(font, size);
  });

  watch(selectedCodeFontSize, (size) => {
    applyCodeFontSettings(size);
  });

  watch(selectedEditorKeymap, (value) => {
    localStorage.setItem('luwiki-editor-keymap', value);
  });

  watch(selectedEditorLineNumbers, (value) => {
    localStorage.setItem('luwiki-editor-line-numbers', value ? '1' : '0');
  });
  watch(selectedDiffMode, (value) => {
    localStorage.setItem('luwiki-diff-mode', value);
  });

  return {
    themeOptions: THEME_OPTIONS,
    fontOptions: FONT_OPTIONS,
    editorKeymapOptions: EDITOR_KEYMAP_OPTIONS,
    diffModeOptions: DIFF_MODE_OPTIONS,
    selectedTheme,
    selectedFont,
    selectedFontSize,
    selectedCodeFontSize,
    selectedEditorKeymap,
    selectedEditorLineNumbers,
    selectedDiffMode,
    codeFontFamily,
    editorStyle,
    markdownThemeClass,
    prismThemeClass,
    markdownStyle,
  };
}

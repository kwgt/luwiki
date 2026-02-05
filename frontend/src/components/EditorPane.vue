<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { Compartment, EditorSelection, EditorState } from '@codemirror/state';
import { EditorView, placeholder } from '@codemirror/view';
import {
  buildBaseExtensions,
  buildLineNumberExtension,
  buildKeymapExtension,
  buildThemeExtension,
  type EditorKeymap,
  type EditorTheme,
} from '../lib/editorExtensions';
import { collectImmediateMacroChanges } from '../lib/macroEngine';

const props = defineProps<{
  modelValue: string;
  theme: EditorTheme;
  keymap: EditorKeymap;
  lineNumbers: boolean;
  macroPagePath?: string;
  macroPageId?: string;
  macroUserId?: string;
  macroUserDisplayName?: string;
  readOnly?: boolean;
  placeholder?: string;
  editorStyle?: Record<string, string>;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const hostRef = ref<HTMLDivElement | null>(null);
const viewRef = ref<EditorView | null>(null);
const themeCompartment = new Compartment();
const keymapCompartment = new Compartment();
const lineNumberCompartment = new Compartment();
const applyingImmediateMacro = ref(false);

function buildEditorState(): EditorState {
  const placeholderExtension = props.placeholder
    ? placeholder(props.placeholder)
    : [];
  const readOnlyExtension = props.readOnly
    ? [EditorState.readOnly.of(true)]
    : [];

  return EditorState.create({
    doc: props.modelValue,
    extensions: [
      ...buildBaseExtensions(),
      themeCompartment.of(buildThemeExtension(props.theme)),
      keymapCompartment.of(buildKeymapExtension(props.keymap)),
      lineNumberCompartment.of(buildLineNumberExtension(props.lineNumbers)),
      ...readOnlyExtension,
      placeholderExtension,
      EditorView.updateListener.of((update) => {
        if (!update.docChanged) {
          return;
        }
        if (!props.readOnly && !applyingImmediateMacro.value) {
          const fullText = update.state.doc.toString();
          const changes = collectImmediateMacroChanges(fullText, {
            pagePath: props.macroPagePath ?? '/',
            pageId: props.macroPageId,
            userId: props.macroUserId,
            userDisplayName: props.macroUserDisplayName,
          });
          if (changes.length > 0) {
            applyingImmediateMacro.value = true;
            update.view.dispatch({
              changes: changes
                .sort((left, right) => right.from - left.from)
                .map((change) => ({
                  from: change.from,
                  to: change.to,
                  insert: change.insert,
                })),
            });
            applyingImmediateMacro.value = false;
            return;
          }
        }
        emit('update:modelValue', update.state.doc.toString());
      }),
    ],
  });
}

onMounted(() => {
  if (!hostRef.value) {
    return;
  }
  viewRef.value = new EditorView({
    state: buildEditorState(),
    parent: hostRef.value,
  });
});

onBeforeUnmount(() => {
  viewRef.value?.destroy();
  viewRef.value = null;
});

watch(
  () => props.modelValue,
  (value) => {
    const view = viewRef.value;
    if (!view) {
      return;
    }
    const current = view.state.doc.toString();
    if (value === current) {
      return;
    }
    view.dispatch({
      changes: { from: 0, to: current.length, insert: value },
    });
  },
);

watch(
  () => props.theme,
  (value) => {
    const view = viewRef.value;
    if (!view) {
      return;
    }
    view.dispatch({
      effects: themeCompartment.reconfigure(buildThemeExtension(value)),
    });
  },
);

watch(
  () => props.lineNumbers,
  (value) => {
    const view = viewRef.value;
    if (!view) {
      return;
    }
    view.dispatch({
      effects: lineNumberCompartment.reconfigure(buildLineNumberExtension(value)),
    });
  },
);

watch(
  () => props.keymap,
  (value) => {
    const view = viewRef.value;
    if (!view) {
      return;
    }
    view.dispatch({
      effects: keymapCompartment.reconfigure(buildKeymapExtension(value)),
    });
  },
);

function focusToStart(): void {
  const view = viewRef.value;
  if (!view) {
    return;
  }
  view.dispatch({
    selection: EditorSelection.cursor(0),
    scrollIntoView: true,
  });
  view.focus();
}

defineExpose({ focusToStart });
</script>

<template>
  <div ref="hostRef" class="h-full w-full" :style="editorStyle"></div>
</template>

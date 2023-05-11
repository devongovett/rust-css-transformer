import * as localWasm from '../../wasm';
import { EditorView, basicSetup } from 'codemirror';
import { javascript } from '@codemirror/lang-javascript';
import { css } from '@codemirror/lang-css';
import { oneDark } from '@codemirror/theme-one-dark';
import { syntaxTree } from '@codemirror/language';
import { linter, lintGutter } from '@codemirror/lint'
import { Compartment } from '@codemirror/state'

const linterCompartment = new Compartment;
const visitorLinterCompartment = new Compartment;

let wasm;
let editor, visitorEditor, outputEditor, modulesEditor, depsEditor;
let enc = new TextEncoder();
let dec = new TextDecoder();
let inputs = document.querySelectorAll('input[type=number]');

async function loadVersions() {
  const { versions } = await fetch('https://data.jsdelivr.com/v1/package/npm/lightningcss-wasm').then(r => r.json());
  versions
    .map(v => {
      const option = document.createElement('option');
      option.value = v;
      option.textContent = v;
      return option;
    })
    .forEach(o => {
      version.appendChild(o);
    })
}

async function loadWasm() {
  if (version.value === 'local') {
    wasm = localWasm;
  } else {
    wasm = await new Function('version', 'return import(`https://cdn.jsdelivr.net/npm/lightningcss-wasm@${version}/lightningcss_node.js`)')(version.value);
  }
  await wasm.default();
}

function loadPlaygroundState() {
  const hash = window.location.hash.slice(1);
  try {
    return JSON.parse(decodeURIComponent(hash));
  } catch {
    return {
      minify: minify.checked,
      nesting: nesting.checked,
      visitorEnabled: visitorEnabled.checked,
      targets: getTargets(),
      source: `@custom-media --modern (color), (hover);

.foo {
  background: yellow;

  -webkit-border-radius: 2px;
  -moz-border-radius: 2px;
  border-radius: 2px;
  
  -webkit-transition: background 200ms;
  -moz-transition: background 200ms;
  transition: background 200ms;

  &.bar {
    color: green;
  }
}

@media (--modern) and (width > 1024px) {
  .a {
    color: green;
  }
}`,
      version: version.value,
      visitor: `{
  Color(color) {
    if (color.type === 'rgb') {
      color.g = 0;
      return color;
    }
  }
}`
    };
  }
}

function reflectPlaygroundState(playgroundState) {
  if (typeof playgroundState.minify !== 'undefined') {
    minify.checked = playgroundState.minify;
  }

  if (typeof playgroundState.cssModules !== 'undefined') {
    cssModules.checked = playgroundState.cssModules;
    compiledModules.hidden = !playgroundState.cssModules;
  }

  if (typeof playgroundState.analyzeDependencies !== 'undefined') {
    analyzeDependencies.checked = playgroundState.analyzeDependencies;
    compiledDependencies.hidden = !playgroundState.analyzeDependencies;
  }

  if (typeof playgroundState.nesting !== 'undefined') {
    nesting.checked = playgroundState.nesting;
  }

  if (typeof playgroundState.customMedia !== 'undefined') {
    customMedia.checked = playgroundState.customMedia;
  }

  if (typeof playgroundState.scope !== 'undefined') {
    scope.checked = playgroundState.scope;
  }

  if (typeof playgroundState.visitorEnabled !== 'undefined') {
    visitorEnabled.checked = playgroundState.visitorEnabled;
  }

  if (playgroundState.targets) {
    const { targets } = playgroundState;
    for (let input of inputs) {
      let value = targets[input.id];
      input.value = value == null ? '' : value >> 16;
    }
  }

  if (playgroundState.unusedSymbols) {
    unusedSymbols.value = playgroundState.unusedSymbols.join('\n');
  }

  if (playgroundState.version) {
    version.value = playgroundState.version;
  }
}

function savePlaygroundState() {
  const playgroundState = {
    minify: minify.checked,
    nesting: nesting.checked,
    customMedia: customMedia.checked,
    scope: scope.checked,
    cssModules: cssModules.checked,
    analyzeDependencies: analyzeDependencies.checked,
    targets: getTargets(),
    source: editor.state.doc.toString(),
    visitorEnabled: visitorEnabled.checked,
    visitor: visitorEditor.state.doc.toString(),
    unusedSymbols: unusedSymbols.value.split('\n').map(v => v.trim()).filter(Boolean),
    version: version.value,
  };

  const hash = encodeURIComponent(JSON.stringify(playgroundState));

  if (
    typeof URL === 'function' &&
    typeof history === 'object' &&
    typeof history.replaceState === 'function'
  ) {
    const url = new URL(location.href);
    url.hash = hash;
    history.replaceState(null, null, url);
  } else {
    location.hash = hash;
  }
}

function getTargets() {
  let targets = {};
  for (let input of inputs) {
    if (input.value !== '') {
      targets[input.id] = input.valueAsNumber << 16;
    }
  }

  return targets;
}

function update() {
  const { transform } = wasm;

  const targets = getTargets();
  try {
    let res = transform({
      filename: 'test.css',
      code: enc.encode(editor.state.doc.toString()),
      minify: minify.checked,
      targets: Object.keys(targets).length === 0 ? null : targets,
      drafts: {
        nesting: nesting.checked,
        customMedia: customMedia.checked,
        scope: scope.checked,
      },
      cssModules: cssModules.checked,
      analyzeDependencies: analyzeDependencies.checked,
      unusedSymbols: unusedSymbols.value.split('\n').map(v => v.trim()).filter(Boolean),
      visitor: visitorEnabled.checked ? (0, eval)('(' + visitorEditor.state.doc.toString() + ')') : undefined,
    });

    let update = outputEditor.state.update({ changes: { from: 0, to: outputEditor.state.doc.length, insert: dec.decode(res.code) } });
    outputEditor.update([update]);

    if (res.exports) {
      let update = modulesEditor.state.update({ changes: { from: 0, to: modulesEditor.state.doc.length, insert: '// CSS module exports\n' + JSON.stringify(res.exports, false, 2) } });
      modulesEditor.update([update]);
    }

    if (res.dependencies) {
      let update = depsEditor.state.update({ changes: { from: 0, to: depsEditor.state.doc.length, insert: '// Dependencies\n' + JSON.stringify(res.dependencies, false, 2) } });
      depsEditor.update([update]);
    }

    compiledModules.hidden = !cssModules.checked;
    compiledDependencies.hidden = !analyzeDependencies.checked;
    visitor.hidden = !visitorEnabled.checked;
    source.dataset.expanded = visitor.hidden;
    compiled.dataset.expanded = compiledModules.hidden && compiledDependencies.hidden;
    compiledModules.dataset.expanded = compiledDependencies.hidden;
    compiledDependencies.dataset.expanded = compiledModules.hidden;

    editor.dispatch({
      effects: linterCompartment.reconfigure(createCssLinter(null, res.warnings))
    });

    visitorEditor.dispatch({
      effects: visitorLinterCompartment.reconfigure(createVisitorLinter(null))
    });
  } catch (e) {
    let update = outputEditor.state.update({ changes: { from: 0, to: outputEditor.state.doc.length, insert: `/* ERROR: ${e.message} */` } });
    outputEditor.update([update]);

    editor.dispatch({
      effects: linterCompartment.reconfigure(createCssLinter(e))
    });

    visitorEditor.dispatch({
      effects: visitorLinterCompartment.reconfigure(createVisitorLinter(e))
    });
  }

  savePlaygroundState();
}

function createCssLinter(lastError, warnings) {
  return linter(view => {
    let diagnostics = [];
    if (lastError && lastError.loc) {
      let l = view.state.doc.line(lastError.loc.line);
      let loc = l.from + lastError.loc.column - 1;
      let node = syntaxTree(view.state).resolveInner(loc, 1);
      diagnostics.push(
        {
          from: node.from,
          to: node.to,
          message: lastError.message,
          severity: 'error'
        }
      );
    }
    if (warnings) {
      for (let warning of warnings) {
        let l = view.state.doc.line(warning.loc.line);
        let loc = l.from + warning.loc.column - 1;
        let node = syntaxTree(view.state).resolveInner(loc, 1);
        diagnostics.push({
          from: node.from,
          to: node.to,
          message: warning.message,
          severity: 'warning'
        });
      }
    }
    return diagnostics;
  }, { delay: 0 });
}

function createVisitorLinter(lastError) {
  return linter(view => {
    if (lastError && !lastError.loc) {
      // Firefox has lineNumber and columnNumber, Safari has line and column.
      let line = lastError.lineNumber ?? lastError.line;
      let column = lastError.columnNumber ?? lastError.column;
      if (lastError.column != null) {
        column--;
      }

      if (line == null) {
        // Chrome.
        let match = lastError.stack.match(/(?:(?:eval.*<anonymous>:)|(?:eval:))(?<line>\d+):(?<column>\d+)/);
        if (match) {
          line = Number(match.groups.line);
          column = Number(match.groups.column);

          // Chrome's column numbers are off by the amount of leading whitespace in the line.
          let l = view.state.doc.line(line);
          let m = l.text.match(/^\s*/);
          if (m) {
            column += m[0].length;
          }
        }
      }

      if (line != null) {
        let l = view.state.doc.line(line);
        let loc = l.from + column;
        let node = syntaxTree(view.state).resolveInner(loc, -1);
        return [
          {
            from: node.from,
            to: node.to,
            message: lastError.message,
            severity: 'error'
          }
        ];
      }
    }
    return [];
  }, { delay: 0 });
}

async function main() {
  await loadWasm();

  let state = loadPlaygroundState();
  reflectPlaygroundState(state);

  editor = new EditorView({
    extensions: [lintGutter(), basicSetup, css(), oneDark, linterCompartment.of(createCssLinter())],
    parent: source,
    doc: state.source,
    dispatch(tr) {
      editor.update([tr]);
      if (tr.docChanged) {
        update();
      }
    }
  });

  visitorEditor = new EditorView({
    extensions: [lintGutter(), basicSetup, javascript(), oneDark, visitorLinterCompartment.of(createVisitorLinter())],
    parent: visitor,
    doc: state.visitor,
    dispatch(tr) {
      visitorEditor.update([tr]);
      if (tr.docChanged) {
        update();
      }
    }
  });

  outputEditor = new EditorView({
    extensions: [basicSetup, css(), oneDark, EditorView.editable.of(false), EditorView.lineWrapping],
    parent: compiled,
  });

  modulesEditor = new EditorView({
    extensions: [basicSetup, javascript(), oneDark, EditorView.editable.of(false), EditorView.lineWrapping],
    parent: compiledModules,
  });

  depsEditor = new EditorView({
    extensions: [basicSetup, javascript(), oneDark, EditorView.editable.of(false), EditorView.lineWrapping],
    parent: compiledDependencies,
  });

  update();
  unusedSymbols.oninput = update;
  for (let input of document.querySelectorAll('input')) {
    input.oninput = update;
  }

  await loadVersions();
  version.onchange = async () => {
    await loadWasm();
    update();
  };
}

main();

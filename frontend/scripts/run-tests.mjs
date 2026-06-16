import {
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, '..');
const outDir = path.join(rootDir, '.tmp-test');

function resolveTscCommand() {
  const binName = process.platform === 'win32' ? 'tsc.cmd' : 'tsc';
  return path.join(rootDir, 'node_modules', '.bin', binName);
}

function collectTestFiles(dirPath) {
  const entries = readdirSync(dirPath, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const resolvedPath = path.join(dirPath, entry.name);
    if (entry.isDirectory()) {
      files.push(...collectTestFiles(resolvedPath));
      continue;
    }
    if (entry.isFile() && entry.name.endsWith('.test.js')) {
      files.push(resolvedPath);
    }
  }

  return files;
}

function collectJavaScriptFiles(dirPath) {
  const entries = readdirSync(dirPath, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const resolvedPath = path.join(dirPath, entry.name);
    if (entry.isDirectory()) {
      files.push(...collectJavaScriptFiles(resolvedPath));
      continue;
    }
    if (entry.isFile() && entry.name.endsWith('.js')) {
      files.push(resolvedPath);
    }
  }

  return files;
}

function rewriteRelativeImports(filePath) {
  const source = readFileSync(filePath, 'utf8');
  const updated = source
    .replace(
      /(from\s+['"])(\.\.?\/[^'".]+)(['"])/g,
      '$1$2.js$3',
    )
    .replace(
      /(import\s*\(\s*['"])(\.\.?\/[^'".]+)(['"]\s*\))/g,
      '$1$2.js$3',
    );

  if (updated !== source) {
    writeFileSync(filePath, updated);
  }
}

rmSync(outDir, { recursive: true, force: true });

const tscResult = spawnSync(
  resolveTscCommand(),
  ['-p', 'tsconfig.test.run.json'],
  {
    cwd: rootDir,
    stdio: 'inherit',
  },
);

if (tscResult.status !== 0) {
  process.exit(tscResult.status ?? 1);
}

mkdirSync(outDir, { recursive: true });
writeFileSync(
  path.join(outDir, 'package.json'),
  JSON.stringify({ type: 'module' }, null, 2),
);

for (const jsFile of collectJavaScriptFiles(outDir)) {
  rewriteRelativeImports(jsFile);
}

const testFiles = collectTestFiles(outDir).sort();

if (testFiles.length === 0) {
  console.error('No compiled test files found.');
  process.exit(1);
}

const nodeResult = spawnSync(
  process.execPath,
  ['--test', ...testFiles],
  {
    cwd: rootDir,
    stdio: 'inherit',
  },
);

process.exit(nodeResult.status ?? 1);

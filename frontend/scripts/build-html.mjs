import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { writeFileSync } from 'node:fs';
import pug from 'pug';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, '..');
const templatePath = path.join(rootDir, 'templates', 'index.pug');
const outputPath = path.join(rootDir, 'index.html');
const html = pug.renderFile(templatePath, { pretty: true });
writeFileSync(outputPath, html);

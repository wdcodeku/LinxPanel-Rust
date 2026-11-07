import type { Extension } from '@codemirror/state'
import { StreamLanguage } from '@codemirror/language'

export interface LanguageOption { id: string; label: string }

/** Choices for the language menu, roughly by how often they appear on servers. */
export const LANGUAGES: LanguageOption[] = [
  { id: 'text', label: 'Plain Text' },
  { id: 'nginx', label: 'Nginx' },
  { id: 'shell', label: 'Shell' },
  { id: 'ini', label: 'INI / Conf' },
  { id: 'yaml', label: 'YAML' },
  { id: 'json', label: 'JSON' },
  { id: 'toml', label: 'TOML' },
  { id: 'xml', label: 'XML' },
  { id: 'html', label: 'HTML' },
  { id: 'css', label: 'CSS' },
  { id: 'javascript', label: 'JavaScript' },
  { id: 'typescript', label: 'TypeScript' },
  { id: 'vue', label: 'Vue' },
  { id: 'php', label: 'PHP' },
  { id: 'python', label: 'Python' },
  { id: 'sql', label: 'SQL' },
  { id: 'markdown', label: 'Markdown' },
  { id: 'go', label: 'Go' },
  { id: 'rust', label: 'Rust' },
  { id: 'java', label: 'Java' },
  { id: 'kotlin', label: 'Kotlin' },
  { id: 'csharp', label: 'C#' },
  { id: 'cpp', label: 'C / C++' },
  { id: 'ruby', label: 'Ruby' },
  { id: 'lua', label: 'Lua' },
  { id: 'perl', label: 'Perl' },
  { id: 'swift', label: 'Swift' },
  { id: 'dockerfile', label: 'Dockerfile' },
  { id: 'powershell', label: 'PowerShell' },
  { id: 'diff', label: 'Diff' },
]

const BY_NAME: Record<string, string> = {
  dockerfile: 'dockerfile', makefile: 'shell', '.bashrc': 'shell', '.profile': 'shell', '.zshrc': 'shell',
  '.env': 'ini', '.htaccess': 'nginx', 'nginx.conf': 'nginx', 'php.ini': 'ini', 'my.cnf': 'ini',
  crontab: 'shell', 'cargo.lock': 'toml', 'go.mod': 'go', '.gitignore': 'ini', '.editorconfig': 'ini',
}
const BY_EXT: Record<string, string> = {
  js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'javascript',
  ts: 'typescript', tsx: 'typescript', mts: 'typescript', vue: 'vue',
  json: 'json', jsonc: 'json', map: 'json', webmanifest: 'json',
  html: 'html', htm: 'html', tpl: 'html', twig: 'html', blade: 'html',
  css: 'css', scss: 'css', less: 'css',
  php: 'php', phtml: 'php',
  py: 'python', pyw: 'python',
  sql: 'sql',
  xml: 'xml', svg: 'xml', xsd: 'xml', plist: 'xml', csproj: 'xml', fsproj: 'xml', config: 'xml',
  md: 'markdown', markdown: 'markdown',
  yml: 'yaml', yaml: 'yaml', neon: 'yaml',
  toml: 'toml',
  ini: 'ini', cnf: 'ini', properties: 'ini', env: 'ini', service: 'ini', timer: 'ini', socket: 'ini',
  sh: 'shell', bash: 'shell', zsh: 'shell', ksh: 'shell',
  conf: 'nginx', vhost: 'nginx',
  go: 'go', rs: 'rust', java: 'java', kt: 'kotlin', kts: 'kotlin', cs: 'csharp',
  c: 'cpp', h: 'cpp', cc: 'cpp', cpp: 'cpp', cxx: 'cpp', hpp: 'cpp',
  rb: 'ruby', gemspec: 'ruby', lua: 'lua', pl: 'perl', pm: 'perl', swift: 'swift',
  ps1: 'powershell', psm1: 'powershell', diff: 'diff', patch: 'diff',
}

/** Best language for a path; nginx for *.conf only under an nginx/vhost folder. */
export function detectLanguage(path: string, content = ''): string {
  const name = (path.split('/').pop() || '').toLowerCase()
  if (BY_NAME[name]) return BY_NAME[name]
  if (name.startsWith('dockerfile')) return 'dockerfile'
  const dot = name.lastIndexOf('.')
  const ext = dot > 0 ? name.slice(dot + 1) : ''
  if (ext === 'conf') {
    // nginx syntax ("server {", "location / {") wherever the file lives.
    if (/nginx|openresty|vhost|sites-|rewrite|proxy/i.test(path)) return 'nginx'
    if (/^\s*(server|http|events|location|upstream|stream|map)\b[^\n]*\{/m.test(content)) return 'nginx'
    return 'ini'
  }
  if (BY_EXT[ext]) return BY_EXT[ext]
  // Scripts without an extension: trust the shebang.
  const shebang = content.slice(0, 120).split('\n')[0]
  if (shebang.startsWith('#!')) {
    if (/python/.test(shebang)) return 'python'
    if (/node|deno|bun/.test(shebang)) return 'javascript'
    if (/php/.test(shebang)) return 'php'
    if (/ruby/.test(shebang)) return 'ruby'
    if (/perl/.test(shebang)) return 'perl'
    if (/lua/.test(shebang)) return 'lua'
    return 'shell'
  }
  return 'text'
}

/** Loads a language's support on demand (each is its own chunk). */
export async function loadLanguage(id: string): Promise<Extension> {
  switch (id) {
    case 'javascript': return (await import('@codemirror/lang-javascript')).javascript({ jsx: true })
    case 'typescript': return (await import('@codemirror/lang-javascript')).javascript({ typescript: true, jsx: true })
    case 'vue':
    case 'html': return (await import('@codemirror/lang-html')).html()
    case 'json': return (await import('@codemirror/lang-json')).json()
    case 'css': return (await import('@codemirror/lang-css')).css()
    case 'php': return (await import('@codemirror/lang-php')).php()
    case 'python': return (await import('@codemirror/lang-python')).python()
    case 'sql': return (await import('@codemirror/lang-sql')).sql()
    case 'xml': return (await import('@codemirror/lang-xml')).xml()
    case 'markdown': return (await import('@codemirror/lang-markdown')).markdown()
    case 'yaml': return (await import('@codemirror/lang-yaml')).yaml()
    case 'go': return (await import('@codemirror/lang-go')).go()
    case 'rust': return (await import('@codemirror/lang-rust')).rust()
    case 'java': return (await import('@codemirror/lang-java')).java()
    case 'cpp': return (await import('@codemirror/lang-cpp')).cpp()
    case 'nginx': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/nginx')).nginx)
    case 'shell': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/shell')).shell)
    case 'ini': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/properties')).properties)
    case 'toml': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/toml')).toml)
    case 'dockerfile': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/dockerfile')).dockerFile)
    case 'ruby': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/ruby')).ruby)
    case 'lua': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/lua')).lua)
    case 'perl': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/perl')).perl)
    case 'swift': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/swift')).swift)
    case 'powershell': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/powershell')).powerShell)
    case 'diff': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/diff')).diff)
    case 'csharp': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/clike')).csharp)
    case 'kotlin': return StreamLanguage.define((await import('@codemirror/legacy-modes/mode/clike')).kotlin)
    default: return []
  }
}

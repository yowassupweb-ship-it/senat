<script lang="ts">
  // Renders a change as a diff: from a unified diff text, git-style structured hunks, or old/new strings.
  let { unified = '', hunks = null, oldStr = '', newStr = '' }: { unified?: string; hunks?: any; oldStr?: string; newStr?: string } = $props();

  type Line = { t: 'add' | 'del' | 'ctx' | 'hunk' | 'meta'; s: string };
  const lines = $derived.by<Line[]>(() => {
    const out: Line[] = [];
    if (unified) {
      for (const l of unified.split('\n')) {
        if (l.startsWith('+++') || l.startsWith('---') || l.startsWith('diff ') || l.startsWith('index ') || l.startsWith('new file') || l.startsWith('deleted file')) out.push({ t: 'meta', s: l });
        else if (l.startsWith('@@')) out.push({ t: 'hunk', s: l });
        else if (l.startsWith('+')) out.push({ t: 'add', s: l.slice(1) });
        else if (l.startsWith('-')) out.push({ t: 'del', s: l.slice(1) });
        else out.push({ t: 'ctx', s: l.startsWith(' ') ? l.slice(1) : l });
      }
    } else if (Array.isArray(hunks) && hunks.length) {
      for (const h of hunks) {
        out.push({ t: 'hunk', s: `@@ -${h.oldStart},${h.oldLines} +${h.newStart},${h.newLines} @@` });
        for (const l of h.lines ?? []) {
          if (l.startsWith('+')) out.push({ t: 'add', s: l.slice(1) });
          else if (l.startsWith('-')) out.push({ t: 'del', s: l.slice(1) });
          else out.push({ t: 'ctx', s: l.slice(1) });
        }
      }
    } else {
      for (const l of oldStr ? oldStr.split('\n') : []) out.push({ t: 'del', s: l });
      for (const l of newStr ? newStr.split('\n') : []) out.push({ t: 'add', s: l });
    }
    return out;
  });
</script>

<div class="diff mono" role="figure" aria-label="Изменения">
  {#each lines as l, i (i)}
    <div class="l {l.t}">
      <span class="sign" aria-hidden="true">{l.t === 'add' ? '+' : l.t === 'del' ? '−' : ''}</span><span class="code">{l.s || ' '}</span>
    </div>
  {/each}
</div>

<style>
  .diff {
    font-size: 12px;
    line-height: 1.55;
    background: var(--bg-sunken);
    border-radius: var(--radius-s);
    overflow: auto;
    max-height: 340px;
    padding: 4px 0;
  }
  .l {
    display: flex;
    white-space: pre;
    min-width: max-content;
  }
  .sign {
    width: 22px;
    flex: none;
    text-align: center;
    color: var(--fg-3);
    user-select: none;
  }
  .add {
    background: var(--diff-add);
  }
  .add .sign {
    color: var(--ok);
  }
  .del {
    background: var(--diff-del);
  }
  .del .sign {
    color: var(--bad);
  }
  .hunk {
    color: var(--accent);
    opacity: 0.85;
    padding: 2px 8px;
  }
  .meta {
    color: var(--fg-3);
    padding: 0 8px;
  }
  .ctx .code {
    color: var(--fg-2);
  }
</style>

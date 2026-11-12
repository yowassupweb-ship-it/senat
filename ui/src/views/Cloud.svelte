<script lang="ts">
  import { del, get, post, postRaw, put } from '../lib/api';
  import { ctx, type MenuItem } from '../lib/contextmenu.svelte';
  import { dateTime } from '../lib/format';
  import Icon from '../components/Icon.svelte';
  import Page from '../components/Page.svelte';

  // The cloud: the files of this environment in an S3 bucket (like MBOX's storage). Specialists and the person put
  // documents here, take them back by a link that expires.
  interface Folder { prefix: string; name: string }
  interface FileObj { name: string; key: string; size: number; modified: string }

  let cfg = $state<any>(null);
  let prefix = $state('');
  let folders = $state<Folder[]>([]);
  let files = $state<FileObj[]>([]);
  let loading = $state(false);
  let error = $state('');
  let note = $state('');
  let editing = $state(false);
  let drag = $state(false);
  let busy = $state('');
  let input = $state<HTMLInputElement>();
  let form = $state({ endpoint: 'https://storage.yandexcloud.net', region: 'ru-central1', bucket: '', access_key: '', secret_key: '', prefix: 'senate/' });

  const crumbs = $derived.by(() => {
    const parts = prefix.split('/').filter(Boolean);
    return parts.map((p, i) => ({ name: p, prefix: parts.slice(0, i + 1).join('/') + '/' }));
  });

  async function loadConfig() {
    try {
      cfg = await get('/api/cloud/config');
      if (cfg.configured) {
        form = { endpoint: cfg.endpoint, region: cfg.region || 'ru-central1', bucket: cfg.bucket, access_key: cfg.access_key, secret_key: '', prefix: cfg.prefix };
        await load();
      } else {
        editing = true;
      }
    } catch (e: any) {
      error = String(e.message || e).includes('JSON') ? 'Эта нода пока не умеет работать с облаком: обновите ноду.' : e.message;
    }
  }
  async function load(p = prefix) {
    loading = true;
    error = '';
    try {
      const r = await get(`/api/cloud?prefix=${encodeURIComponent(p)}`);
      if (!r.configured) {
        editing = true;
        return;
      }
      prefix = r.prefix;
      folders = r.folders;
      files = r.files.sort((a: FileObj, b: FileObj) => a.name.localeCompare(b.name, 'ru'));
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    loadConfig();
  });

  async function saveConfig(e: Event) {
    e.preventDefault();
    error = '';
    try {
      cfg = await put('/api/cloud/config', form);
      form.secret_key = '';
      editing = false;
      prefix = '';
      await load('');
    } catch (err: any) {
      error = err.message;
    }
  }

  async function uploadFiles(list: FileList | File[]) {
    error = '';
    for (const f of Array.from(list)) {
      busy = `Загружаю ${f.name}…`;
      try {
        await postRaw(`/api/cloud/upload?prefix=${encodeURIComponent(prefix)}&name=${encodeURIComponent(f.name)}`, f);
      } catch (e: any) {
        error = `${f.name}: ${e.message}`;
        break;
      }
    }
    busy = '';
    await load();
  }
  function onDrop(e: DragEvent) {
    e.preventDefault();
    drag = false;
    if (e.dataTransfer?.files?.length) uploadFiles(e.dataTransfer.files);
  }
  async function newFolder() {
    const name = prompt('Название папки');
    if (!name?.trim()) return;
    try {
      await post('/api/cloud/folder', { prefix, name });
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }
  async function linkFor(key: string, minutes = 60): Promise<string> {
    return (await get(`/api/cloud/link?key=${encodeURIComponent(key)}&minutes=${minutes}`)).url;
  }
  async function download(f: FileObj) {
    try {
      const a = document.createElement('a');
      a.href = await linkFor(f.key, 10);
      a.target = '_blank';
      a.rel = 'noopener';
      a.click();
    } catch (e: any) {
      error = e.message;
    }
  }
  async function copyLink(f: FileObj, minutes: number) {
    try {
      await navigator.clipboard.writeText(await linkFor(f.key, minutes));
      note = minutes >= 1440 ? 'Ссылка скопирована, действует 7 дней' : 'Ссылка скопирована, действует час';
      setTimeout(() => (note = ''), 3500);
    } catch (e: any) {
      error = e.message;
    }
  }
  async function remove(key: string, label: string) {
    if (!confirm(`Удалить «${label}»${key.endsWith('/') ? ' вместе со всем содержимым' : ''}?`)) return;
    try {
      await del(`/api/cloud?key=${encodeURIComponent(key)}`);
      await load();
    } catch (e: any) {
      error = e.message;
    }
  }

  const size = (n: number) => (n < 1024 ? `${n} Б` : n < 1048576 ? `${(n / 1024).toFixed(n < 10240 ? 1 : 0)} КБ` : n < 1073741824 ? `${(n / 1048576).toFixed(1)} МБ` : `${(n / 1073741824).toFixed(1)} ГБ`);
  const fileMenu = (f: FileObj): MenuItem[] => [
    { label: 'Скачать', icon: 'download', run: () => download(f) },
    { label: 'Копировать ссылку на час', icon: 'link', run: () => copyLink(f, 60) },
    { label: 'Копировать ссылку на 7 дней', icon: 'link', run: () => copyLink(f, 10080) },
    { sep: true },
    { label: 'Копировать путь', icon: 'copy', run: () => navigator.clipboard?.writeText(f.key) },
    { sep: true },
    { label: 'Удалить', icon: 'trash', danger: true, run: () => remove(f.key, f.name) },
  ];
  const folderMenu = (f: Folder): MenuItem[] => [
    { label: 'Открыть', icon: 'folder', run: () => load(f.prefix) },
    { sep: true },
    { label: 'Удалить папку', icon: 'trash', danger: true, run: () => remove(f.prefix, f.name) },
  ];
</script>

<Page title="Облако" sub="Файлы среды в S3-хранилище: документы, выгрузки, материалы специалистов.">
  {#snippet actions()}
    {#if cfg?.configured && !editing}
      <button class="btn" onclick={newFolder}><Icon name="foldernew" size={14} />Папка</button>
      <button class="btn primary" onclick={() => input?.click()}><Icon name="upload" size={14} />Загрузить</button>
      <button class="icon-btn" onclick={() => (editing = true)} aria-label="Настройки облака" title="Настройки облака"><Icon name="sliders" size={15} /></button>
      <input bind:this={input} type="file" multiple hidden onchange={(e) => { if (e.currentTarget.files?.length) uploadFiles(e.currentTarget.files); e.currentTarget.value = ''; }} />
    {/if}
  {/snippet}

  {#if error}<p class="bad" role="alert">{error}</p>{/if}
  {#if note}<p class="ok" role="status">{note}</p>{/if}

  {#if editing}
    <form class="card pad form" onsubmit={saveConfig}>
      <h3>Подключение к хранилищу</h3>
      <p class="muted">Любое хранилище, совместимое с S3: Яндекс Object Storage, MinIO, AWS. Ключи остаются только на ноде и не показываются.</p>
      <div class="two">
        <label>Адрес<input class="field" bind:value={form.endpoint} required placeholder="https://storage.yandexcloud.net" /></label>
        <label>Регион<input class="field" bind:value={form.region} placeholder="ru-central1" /></label>
      </div>
      <div class="two">
        <label>Бакет<input class="field" bind:value={form.bucket} required placeholder="my-bucket" /></label>
        <label>Папка в бакете <small>всё Senate лежит внутри неё</small><input class="field" bind:value={form.prefix} placeholder="senate/" /></label>
      </div>
      <div class="two">
        <label>Идентификатор ключа<input class="field mono" bind:value={form.access_key} required autocomplete="off" /></label>
        <label>Секретный ключ {#if cfg?.has_secret}<small>оставьте пустым, чтобы не менять</small>{/if}<input class="field mono" type="password" bind:value={form.secret_key} required={!cfg?.has_secret} autocomplete="new-password" /></label>
      </div>
      <div class="row"><button class="btn primary" type="submit">Сохранить и проверить</button>{#if cfg?.configured}<button class="btn" type="button" onclick={() => { editing = false; error = ''; }}>Отмена</button>{/if}</div>
    </form>
  {:else if cfg?.configured}
    <nav class="crumbs" aria-label="Путь">
      <button class:cur={!prefix} onclick={() => load('')}><Icon name="cloud" size={14} />Облако</button>
      {#each crumbs as c, i}<Icon name="right" size={12} /><button class:cur={i === crumbs.length - 1} onclick={() => load(c.prefix)}>{c.name}</button>{/each}
      {#if busy}<span class="faint grow">{busy}</span>{/if}
    </nav>

    <div class="drop" class:drag ondragover={(e) => { e.preventDefault(); drag = true; }} ondragleave={() => (drag = false)} ondrop={onDrop} role="region" aria-label="Файлы: перетащите сюда, чтобы загрузить">
      {#if loading && !folders.length && !files.length}
        <p class="empty muted">Загрузка…</p>
      {:else if !folders.length && !files.length}
        <p class="empty muted">Здесь пока пусто. Перетащите файлы в это окно или нажмите «Загрузить».</p>
      {:else}
        <ul class="rows">
          {#each folders as f (f.prefix)}
            <li use:ctx={() => folderMenu(f)}>
              <button class="name" onclick={() => load(f.prefix)}><Icon name="folder" size={16} /><span>{f.name}</span></button>
              <span class="meta faint">папка</span><span class="meta"></span>
              <button class="icon-btn" aria-label={`Удалить папку ${f.name}`} onclick={() => remove(f.prefix, f.name)}><Icon name="trash" size={14} /></button>
            </li>
          {/each}
          {#each files as f (f.key)}
            <li use:ctx={() => fileMenu(f)}>
              <button class="name" onclick={() => download(f)} title="Скачать"><Icon name="file" size={16} /><span>{f.name}</span></button>
              <span class="meta faint">{size(f.size)}</span><span class="meta faint">{f.modified ? dateTime(Date.parse(f.modified)) : ''}</span>
              <span class="acts">
                <button class="icon-btn" aria-label={`Скопировать ссылку на ${f.name}`} title="Копировать ссылку на час" onclick={() => copyLink(f, 60)}><Icon name="link" size={14} /></button>
                <button class="icon-btn" aria-label={`Удалить ${f.name}`} onclick={() => remove(f.key, f.name)}><Icon name="trash" size={14} /></button>
              </span>
            </li>
          {/each}
        </ul>
      {/if}
      {#if drag}<div class="veil">Отпустите, чтобы загрузить</div>{/if}
    </div>
  {:else if !error}
    <p class="muted">Загрузка…</p>
  {/if}
</Page>

<style>
  .bad { color: var(--bad); margin: 0 0 12px; font-size: 13px; }
  .ok { color: var(--ok); margin: 0 0 12px; font-size: 13px; }
  .form { display: flex; flex-direction: column; gap: 12px; max-width: 760px; }
  .form h3 { margin: 0; font-size: 15px; }
  .form p { margin: 0; font-size: 13px; }
  .form label { display: flex; flex-direction: column; gap: 4px; font-size: 13px; font-weight: 600; }
  .form label small { font-weight: 400; color: var(--fg-3); }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .crumbs { display: flex; align-items: center; gap: 4px; flex-wrap: wrap; margin-bottom: 12px; color: var(--fg-3); }
  .crumbs button { display: inline-flex; align-items: center; gap: 6px; padding: 4px 8px; border-radius: 8px; color: var(--fg-2); font-size: 13.5px; }
  .crumbs button:hover { background: var(--bg-hover); color: var(--fg); }
  .crumbs button.cur { color: var(--fg); font-weight: 600; }
  .grow { margin-left: auto; }
  .drop { position: relative; border-radius: var(--radius); min-height: 220px; }
  .veil { position: absolute; inset: 0; display: grid; place-items: center; border-radius: var(--radius); background: color-mix(in srgb, var(--bg) 82%, transparent); box-shadow: inset 0 0 0 2px var(--fg-3); font-weight: 600; pointer-events: none; }
  .empty { padding: 60px 16px; text-align: center; margin: 0; }
  .rows { list-style: none; margin: 0; padding: 0; background: var(--bg-raised); border-radius: var(--radius); box-shadow: var(--shadow-1); overflow: hidden; }
  .rows li { display: grid; grid-template-columns: minmax(0, 1fr) 90px 150px 70px; align-items: center; gap: 8px; padding: 6px 12px; border-top: 1px solid var(--line); }
  .rows li:first-child { border-top: 0; }
  .rows li:hover { background: var(--bg-hover); }
  .name { display: flex; align-items: center; gap: 10px; min-width: 0; padding: 6px 0; text-align: left; font-size: 14px; }
  .name span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .meta { font-size: 12.5px; text-align: right; white-space: nowrap; }
  .acts { display: flex; justify-content: flex-end; gap: 2px; }
  @media (max-width: 760px) { .two { grid-template-columns: 1fr; } .rows li { grid-template-columns: minmax(0, 1fr) 70px 56px; } .rows li .meta:nth-of-type(2) { display: none; } }
</style>

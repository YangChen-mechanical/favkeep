<script lang="ts">
	import { SvelteURLSearchParams } from 'svelte/reactivity';
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { mode, toggleMode, setMode } from 'mode-watcher';
	import SearchIcon from '@lucide/svelte/icons/search';
	import SunIcon from '@lucide/svelte/icons/sun';
	import MoonIcon from '@lucide/svelte/icons/moon';
	import SettingsIcon from '@lucide/svelte/icons/settings-2';
	import PlayIcon from '@lucide/svelte/icons/play';
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import { Button } from '$lib/components/ui/button/index.js';
	import api from '$lib/api';
	import type { ApiError } from '$lib/types';
	import './watch.css';

	let search = '';
	let ready = false;
	let error = '';
	let unauthorized = false;
	let alive = true;
	$: if ($page.url.pathname === '/watch') search = $page.url.searchParams.get('q') || '';
	async function connect() {
		error = '';
		unauthorized = false;
		try {
			await api.openMediaSession();
			if (alive) ready = true;
		} catch (cause) {
			if (!alive) return;
			unauthorized = (cause as ApiError).status === 401;
			error = unauthorized
				? '完成认证后，即可浏览和观看你的媒体库。'
				: '暂时无法连接媒体库，请稍后重试。';
		}
	}
	onMount(() => {
		if (!localStorage.getItem('mode-watcher-mode')) setMode('light');
		void connect();
		return () => {
			alive = false;
		};
	});
</script>

<div class="watch-shell">
	<header class="watch-header">
		<nav class="watch-nav" aria-label="观看导航">
			<a class="watch-brand" href="/watch" aria-label="favkeep 观看中心"
				><span class="brand-icon"><PlayIcon size={18} fill="currentColor" /></span><span
					>favkeep<span class="brand-dot">.</span></span
				></a
			>
			<form
				class="header-search"
				role="search"
				onsubmit={(event) => {
					event.preventDefault();
					const params = new SvelteURLSearchParams(
						$page.url.pathname === '/watch' ? $page.url.search : ''
					);
					params.delete('page');
					if (search.trim()) params.set('q', search.trim());
					else params.delete('q');
					void goto(`/watch${params.size ? `?${params}` : ''}`);
				}}
			>
				<SearchIcon size={18} aria-hidden="true" />
				<label class="sr-only" for="watch-query">搜索标题、作者或 BV 号</label>
				<input
					id="watch-query"
					type="search"
					bind:value={search}
					placeholder="搜索标题、作者或 BV 号"
					autocomplete="off"
				/>
				<button type="submit" aria-label="搜索">搜索</button>
			</form>
			<div class="header-actions">
				<Button
					variant="ghost"
					size="icon"
					onclick={toggleMode}
					aria-label={mode.current === 'light' ? '切换为暗色' : '切换为亮色'}
					title="切换主题"
				>
					{#if mode.current === 'light'}<MoonIcon size={18} />{:else}<SunIcon size={18} />{/if}
				</Button>
				<Button variant="ghost" href="/" aria-label="进入管理" title="进入管理"
					><SettingsIcon size={17} /><span class="manage-label">管理</span></Button
				>
			</div>
		</nav>
	</header>
	<main class="watch-content" id="main">
		{#if ready}<slot />
		{:else if error}
			<section class="watch-empty" aria-label="媒体库连接">
				<div class="empty-icon"><PlayIcon size={28} /></div>
				<h1>{unauthorized ? '连接你的媒体库' : '连接暂时中断'}</h1>
				<p role="alert">{error}</p>
				{#if unauthorized}<Button href="/settings">前往设置认证<ArrowRightIcon size={16} /></Button
					>{:else}<Button onclick={connect}>重新连接</Button>{/if}
			</section>
		{:else}<div class="connection-loading" role="status">正在连接媒体库…</div>{/if}
	</main>
</div>

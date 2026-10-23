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
	import LogOutIcon from '@lucide/svelte/icons/log-out';
	import { Button } from '$lib/components/ui/button/index.js';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import './watch.css';

	let search = '';
	$: if ($page.url.pathname === '/watch') search = $page.url.searchParams.get('q') || '';
	onMount(() => {
		if (!localStorage.getItem('mode-watcher-mode')) setMode('light');
	});
	async function logout() {
		try {
			await api.logout();
		} catch {
			toast.error('退出失败，请检查连接后重试');
		}
	}
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
				<Button variant="ghost" size="icon" onclick={logout} aria-label="退出登录" title="退出登录"
					><LogOutIcon size={17} /></Button
				>
			</div>
		</nav>
	</header>
	<main class="watch-content" id="main"><slot /></main>
</div>

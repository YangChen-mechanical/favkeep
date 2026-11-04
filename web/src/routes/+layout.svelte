<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import AppSidebar from '$lib/components/app-sidebar.svelte';
	import * as Breadcrumb from '$lib/components/ui/breadcrumb/index.js';
	import { Separator } from '$lib/components/ui/separator/index.js';
	import { Button } from '$lib/components/ui/button/index.js';
	import { breadcrumbStore } from '$lib/stores/breadcrumb';
	import { authSession, returnPath } from '$lib/stores/auth';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import { Toaster } from '$lib/components/ui/sonner/index.js';
	import { ModeWatcher } from 'mode-watcher';
	import api from '$lib/api';
	import { loadPlatforms } from '$lib/stores/platforms';
	import { toast } from 'svelte-sonner';
	let checked = false;
	let catalogLoaded = false;
	$: if (checked && $authSession?.authenticated && !catalogLoaded) {
		void ensureCatalog();
	}
	async function ensureCatalog() {
		catalogLoaded = true;
		try {
			await loadPlatforms();
		} catch {
			if ($authSession?.authenticated) sessionError = '平台列表加载失败，请重试。';
		}
	}
	$: if (!$authSession?.authenticated) catalogLoaded = false;
	let sessionError = '';
	$: watching = $page.url.pathname === '/watch' || $page.url.pathname.startsWith('/watch/');
	$: authPage = ['/login', '/setup'].includes($page.url.pathname);
	$: if (checked && $authSession) {
		const path = $page.url.pathname;
		if ($authSession.setup_required && path !== '/setup') {
			void goto('/setup', { replaceState: true });
		} else if (!$authSession.setup_required && !$authSession.authenticated && path !== '/login') {
			const destination = authPage ? '/watch' : path + $page.url.search;
			void goto('/login?returnTo=' + encodeURIComponent(destination), { replaceState: true });
		} else if ($authSession.authenticated && authPage) {
			void goto(returnPath($page.url.searchParams.get('returnTo')), { replaceState: true });
		}
	}
	async function checkSession() {
		sessionError = '';
		try {
			const session = await api.getSession();
			if (session.data.authenticated) {
				catalogLoaded = true;
				await ensureCatalog();
			}
			checked = true;
		} catch {
			sessionError = '无法连接认证服务，请检查连接后重试。';
		}
	}
	async function logout() {
		try {
			await api.logout();
		} catch {
			toast.error('退出失败，请检查连接后重试');
		}
	}
	onMount(() => {
		void checkSession();
	});
</script>

<ModeWatcher disableHeadScriptInjection />
<Toaster position="top-center" duration={3000} />
{#if sessionError}
	<main class="mx-auto max-w-lg space-y-4 p-6">
		<p role="alert">{sessionError}</p>
		<Button onclick={checkSession}>重试</Button>
	</main>
{:else if !checked}
	<p class="p-6" role="status">正在检查登录状态…</p>
{:else if authPage}
	<slot />
{:else if $authSession?.authenticated}
	{#if watching}
		<slot />
	{:else}
		<Sidebar.Provider>
			<AppSidebar />
			<Sidebar.Inset class="flex flex-col" style="height: calc(100vh - 1rem)">
				<header class="flex h-16 shrink-0 items-center justify-between gap-2 px-4">
					<div class="flex items-center gap-2">
						<Sidebar.Trigger class="-ml-1" />
						<Separator orientation="vertical" class="mr-2 data-[orientation=vertical]:h-4" />
						<Breadcrumb.Root>
							<Breadcrumb.List>
								{#each $breadcrumbStore as item, index (item.label)}
									<Breadcrumb.Item class="hidden md:block">
										{#if item.href}
											<Breadcrumb.Link href={item.href}>{item.label}</Breadcrumb.Link>
										{:else}
											<Breadcrumb.Page>{item.label}</Breadcrumb.Page>
										{/if}
									</Breadcrumb.Item>
									{#if index < $breadcrumbStore.length - 1}
										<Breadcrumb.Separator class="hidden md:block" />
									{/if}
								{/each}
							</Breadcrumb.List>
						</Breadcrumb.Root>
					</div>
					<Button variant="ghost" onclick={logout}>退出登录</Button>
				</header>
				<div
					class="w-full overflow-y-auto px-6 py-2"
					style="scrollbar-width: thin; scrollbar-gutter: stable !important;"
					id="main"
				>
					<slot />
				</div>
			</Sidebar.Inset>
		</Sidebar.Provider>
	{/if}
{/if}

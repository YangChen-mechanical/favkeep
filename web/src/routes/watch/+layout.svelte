<script lang="ts">
	import { Button } from '$lib/components/ui/button/index.js';
	import api from '$lib/api';
	import { toast } from 'svelte-sonner';
	import { mode, toggleMode } from 'mode-watcher';
</script>

<div class="bg-background text-foreground min-h-svh">
	<header class="border-b">
		<nav
			aria-label="观看导航"
			class="mx-auto flex max-w-7xl flex-wrap items-center justify-between gap-3 px-4 py-4 sm:px-6"
		>
			<a href="/watch" class="rounded text-xl font-semibold focus-visible:outline-2">favkeep</a>
			<div class="flex flex-wrap items-center gap-2">
				<Button variant="ghost" href="/watch">观看中心</Button>
				<Button variant="ghost" onclick={toggleMode}>
					切换为{mode.current === 'light' ? '暗色' : '亮色'}
				</Button>
				<Button variant="outline" href="/">进入管理</Button>
				<Button
					variant="ghost"
					onclick={async () => {
						try {
							await api.logout();
						} catch {
							toast.error('退出失败，请重试');
						}
					}}>退出登录</Button
				>
			</div>
		</nav>
	</header>
	<main class="mx-auto max-w-7xl px-4 py-6 sm:px-6 sm:py-8" id="main">
		<slot />
	</main>
</div>

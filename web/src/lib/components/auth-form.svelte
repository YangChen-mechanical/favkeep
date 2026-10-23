<script lang="ts">
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import api from '$lib/api';
	import { returnPath } from '$lib/stores/auth';
	import { Button } from '$lib/components/ui/button/index.js';
	import { Input } from '$lib/components/ui/input/index.js';
	import { Label } from '$lib/components/ui/label/index.js';
	import type { ApiError } from '$lib/types';
	export let setup = false;
	let username = '';
	let password = '';
	let confirmation = '';
	let setupCode = '';
	let busy = false;
	let error = '';
	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		error = '';
		if (setup && password !== confirmation) {
			error = '两次密码不一致';
			return;
		}
		busy = true;
		try {
			if (setup) await api.setup(setupCode.trim(), username, password);
			else await api.login(username, password);
			password = '';
			confirmation = '';
			setupCode = '';
			await goto(returnPath($page.url.searchParams.get('returnTo')), { replaceState: true });
		} catch (cause) {
			error = (cause as ApiError).message;
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>{setup ? '初始化管理员' : '登录'} - favkeep</title></svelte:head>
<main class="bg-muted/40 flex min-h-svh items-center justify-center px-4 py-10">
	<div class="bg-card w-full max-w-sm space-y-6 rounded-xl border p-6 shadow-sm">
		<div class="space-y-2">
			<p class="text-muted-foreground text-sm font-medium">favkeep</p>
			<h1 class="text-2xl font-semibold">{setup ? '初始化管理员' : '登录你的媒体库'}</h1>
			<p class="text-muted-foreground text-sm">
				{setup ? '输入服务器上的一次性初始化码，并创建管理员。' : '观看和管理共用此登录。'}
			</p>
		</div>
		<form onsubmit={submit} class="space-y-4">
			{#if setup}<div class="space-y-2">
					<Label for="setup-code">初始化码</Label><Input
						id="setup-code"
						type="password"
						bind:value={setupCode}
						required
						autocomplete="off"
					/>
					<p class="text-muted-foreground text-xs">
						位于服务器配置目录的 auth-setup.secret 文件中。
					</p>
				</div>{/if}
			<div class="space-y-2">
				<Label for="username">用户名</Label><Input
					id="username"
					bind:value={username}
					required
					maxlength={64}
					autocomplete="username"
				/>
			</div>
			<div class="space-y-2">
				<Label for="password">密码</Label><Input
					id="password"
					type="password"
					bind:value={password}
					required
					minlength={setup ? 15 : undefined}
					maxlength={128}
					autocomplete={setup ? 'new-password' : 'current-password'}
				/>{#if setup}<p class="text-muted-foreground text-xs">
						15–128 个字符，可以使用较长的短语。
					</p>{/if}
			</div>
			{#if setup}<div class="space-y-2">
					<Label for="confirm-password">再次输入密码</Label><Input
						id="confirm-password"
						type="password"
						bind:value={confirmation}
						required
						autocomplete="new-password"
					/>
				</div>{/if}
			{#if error}<p role="alert" class="text-destructive text-sm">{error}</p>{/if}
			<Button type="submit" class="w-full" disabled={busy}
				>{busy ? '请稍候…' : setup ? '创建管理员' : '登录'}</Button
			>
		</form>
	</div>
</main>

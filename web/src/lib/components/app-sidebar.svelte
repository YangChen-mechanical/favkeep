<script lang="ts">
	import BotIcon from '@lucide/svelte/icons/bot';
	import ChartPieIcon from '@lucide/svelte/icons/chart-pie';
	import FilePlayIcon from '@lucide/svelte/icons/file-play';
	import { platforms } from '$lib/stores/platforms';
	import { page } from '$app/stores';

	import PaletteIcon from '@lucide/svelte/icons/palette';
	import Settings2Icon from '@lucide/svelte/icons/settings-2';
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import * as Sidebar from '$lib/components/ui/sidebar/index.js';
	import { mode, toggleMode } from 'mode-watcher';
	import type { ComponentProps } from 'svelte';

	let sidebar = Sidebar.useSidebar();

	let { ref = $bindable(null), ...restProps }: ComponentProps<typeof Sidebar.Root> = $props();

	const data = $derived({
		header: {
			title: 'favkeep',
			subtitle: '后台管理系统',
			icon: BotIcon,
			href: '/'
		},
		navMain: [
			{
				category: '总览',
				items: [
					{
						title: '仪表盘',
						icon: ChartPieIcon,
						href: '/'
					},
					{
						title: '日志',
						icon: SquareTerminalIcon,
						href: '/logs'
					}
				]
			},
			{
				category: '平台',
				items: $platforms.map((p) => ({
					title: p.name,
					icon: FilePlayIcon,
					href: `/platforms/${p.id}`
				}))
			},
			{ category: '媒体', items: [{ title: '全部视频', icon: FilePlayIcon, href: '/videos' }] }
		],
		footer: [
			{
				title: '进入观看',
				icon: FilePlayIcon,
				href: '/watch'
			},
			{
				title: '通用设置',
				icon: Settings2Icon,
				href: '/settings'
			}
		]
	});

	function isActive(href: string) {
		const path = $page.url.pathname;
		return path === href || (href === '/platforms/bilibili' && path.startsWith('/me/'));
	}
	const closeMobileSidebar = () => {
		if (sidebar.isMobile) {
			sidebar.setOpenMobile(false);
		}
	};
</script>

<Sidebar.Root bind:ref variant="inset" {...restProps}>
	<Sidebar.Header>
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<Sidebar.MenuButton size="lg">
					{#snippet child({ props })}
						<a href={data.header.href} {...props} onclick={closeMobileSidebar}>
							<div
								class="bg-sidebar-primary text-sidebar-primary-foreground flex aspect-square size-8 items-center justify-center rounded-lg"
							>
								<data.header.icon class="size-4" />
							</div>
							<div class="grid flex-1 text-left text-sm leading-tight">
								<span class="truncate font-medium">{data.header.title}</span>
								<span class="truncate text-xs">{data.header.subtitle}</span>
							</div>
						</a>
					{/snippet}
				</Sidebar.MenuButton>
			</Sidebar.MenuItem>
		</Sidebar.Menu>
	</Sidebar.Header>
	<Sidebar.Content>
		<Sidebar.Group>
			{#each data.navMain as group (group.category)}
				<Sidebar.GroupLabel class="h-10">{group.category}</Sidebar.GroupLabel>
				<Sidebar.Menu>
					{#each group.items as item (item.title)}
						<Sidebar.MenuItem>
							<Sidebar.MenuButton class="h-8" isActive={isActive(item.href)}>
								{#snippet child({ props })}
									<a
										href={item.href}
										{...props}
										aria-current={isActive(item.href) ? 'page' : undefined}
										onclick={closeMobileSidebar}
									>
										<item.icon class="size-4" />
										<span class="text-sm">{item.title}</span>
									</a>
								{/snippet}
							</Sidebar.MenuButton>
						</Sidebar.MenuItem>
					{/each}
				</Sidebar.Menu>
			{/each}
		</Sidebar.Group>
	</Sidebar.Content>
	<Sidebar.Footer>
		<Sidebar.Separator />
		<Sidebar.Menu>
			<Sidebar.MenuItem>
				<Sidebar.MenuButton class="h-8 cursor-pointer">
					{#snippet child({ props })}
						<button
							{...props}
							onclick={() => {
								toggleMode();
								closeMobileSidebar();
							}}
						>
							<PaletteIcon class="size-4" />
							<span class="text-sm">{mode.current === 'light' ? '亮色' : '暗色'}</span>
						</button>
					{/snippet}
				</Sidebar.MenuButton>
			</Sidebar.MenuItem>
			{#each data.footer as item (item.title)}
				<Sidebar.MenuItem>
					<Sidebar.MenuButton class="h-8" isActive={isActive(item.href)}>
						{#snippet child({ props })}
							<a
								href={item.href}
								{...props}
								aria-current={isActive(item.href) ? 'page' : undefined}
								onclick={closeMobileSidebar}
							>
								<item.icon class="size-4" />
								<span class="text-sm">{item.title}</span>
							</a>
						{/snippet}
					</Sidebar.MenuButton>
				</Sidebar.MenuItem>
			{/each}
		</Sidebar.Menu>
	</Sidebar.Footer>
</Sidebar.Root>

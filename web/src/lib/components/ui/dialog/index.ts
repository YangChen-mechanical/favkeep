import { Dialog as DialogPrimitive } from 'bits-ui';

import Content from './dialog-content.svelte';
import Description from './dialog-description.svelte';
import Header from './dialog-header.svelte';
import Overlay from './dialog-overlay.svelte';
import Title from './dialog-title.svelte';

const Root = DialogPrimitive.Root;
const Portal = DialogPrimitive.Portal;

export {
	Content,
	Description,
	//
	Root as Dialog,
	Content as DialogContent,
	Description as DialogDescription,
	Header as DialogHeader,
	Overlay as DialogOverlay,
	Portal as DialogPortal,
	Title as DialogTitle,
	Header,
	Overlay,
	Portal,
	Root,
	Title
};

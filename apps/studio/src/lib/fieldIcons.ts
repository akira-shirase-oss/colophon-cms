// One icon per kind of field, for the builder's rows and the add-a-field picker: the same mark in
// both places, so a field is recognised by its shape before its words are read.

import AtSignIcon from '@lucide/svelte/icons/at-sign';
import BoxIcon from '@lucide/svelte/icons/box';
import BracesIcon from '@lucide/svelte/icons/braces';
import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
import CalendarIcon from '@lucide/svelte/icons/calendar';
import CircleDotIcon from '@lucide/svelte/icons/circle-dot';
import ClockIcon from '@lucide/svelte/icons/clock';
import ComponentIcon from '@lucide/svelte/icons/component';
import FileCodeIcon from '@lucide/svelte/icons/file-code';
import GlobeIcon from '@lucide/svelte/icons/globe';
import HashIcon from '@lucide/svelte/icons/hash';
import LayoutTemplateIcon from '@lucide/svelte/icons/layout-template';
import LinkIcon from '@lucide/svelte/icons/link';
import ListIcon from '@lucide/svelte/icons/list';
import PaletteIcon from '@lucide/svelte/icons/palette';
import PilcrowIcon from '@lucide/svelte/icons/pilcrow';
import SignpostIcon from '@lucide/svelte/icons/signpost';
import ToggleLeftIcon from '@lucide/svelte/icons/toggle-left';
import TypeIcon from '@lucide/svelte/icons/type';
import type { FieldDefinition, FieldType, ItemKind } from './api.ts';

export const FIELD_ICONS: Record<FieldType | 'component', typeof TypeIcon> = {
	text: TypeIcon,
	rich_text: PilcrowIcon,
	markdown: FileCodeIcon,
	number: HashIcon,
	boolean: ToggleLeftIcon,
	enum: CircleDotIcon,
	datetime: CalendarClockIcon,
	date: CalendarIcon,
	time: ClockIcon,
	email: AtSignIcon,
	url: GlobeIcon,
	color: PaletteIcon,
	json: BracesIcon,
	reference: LinkIcon,
	object: BoxIcon,
	list: ListIcon,
	blocks: LayoutTemplateIcon,
	slug: SignpostIcon,
	component: ComponentIcon
};

/** A field's icon: a shared component's own, not a group's. */
export const iconOf = (field: FieldDefinition | ItemKind) =>
	FIELD_ICONS[field.type === 'object' && field.component !== undefined ? 'component' : field.type];

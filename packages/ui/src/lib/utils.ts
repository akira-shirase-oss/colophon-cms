// Shared helpers for the components (the shadcn-svelte conventions): `cn` merges class names,
// resolving Tailwind conflicts, and the types shape component props.
import { createCn } from 'cn/config';

/** Merges class names, resolving conflicts, including the design system's own shadows (D-056):
 *  without them, `shadow-none` couldn't replace `shadow-soft` in a component's classes. */
export const cn = createCn({
	extend: { classGroups: { shadow: [{ shadow: ['hairline', 'soft', 'float', 'press'] }] } }
});

export type WithoutChild<T> = T extends { child?: unknown } ? Omit<T, 'child'> : T;
export type WithoutChildren<T> = T extends { children?: unknown } ? Omit<T, 'children'> : T;
export type WithoutChildrenOrChild<T> = WithoutChildren<WithoutChild<T>>;
export type WithElementRef<T, U extends HTMLElement = HTMLElement> = T & { ref?: U | null };

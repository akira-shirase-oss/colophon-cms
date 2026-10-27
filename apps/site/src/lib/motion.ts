// Small motion helpers for the site's live graphics (no animation library).
//
// A graphic is drawn in its final state by default: that is what the prerendered HTML, a reader
// without JavaScript and a reader who asks for reduced motion all see. `inView` puts a
// `data-run` attribute on its element, which is what switches the CSS animations on:
//   armed  the element is close to the viewport: animations sit on their first frame, paused;
//   on     at least a third of it is visible: they run;
//   off    it has left the viewport: they pause where they are.
// Animations touch only transform, opacity and SVG stroke offsets.
import type { Attachment } from 'svelte/attachments';

const REDUCE = '(prefers-reduced-motion: reduce)';

export function reducedMotion(): boolean {
	return typeof window === 'undefined' || window.matchMedia(REDUCE).matches;
}

export type RunState = 'armed' | 'on' | 'off';

/** `on` once `threshold` of the element is visible (a third by default). */
export function inView(onChange?: (state: RunState) => void, threshold = 0.3): Attachment<HTMLElement> {
	return (node) => {
		if (reducedMotion() || !('IntersectionObserver' in window)) return;
		let state: RunState | null = null;
		const set = (next: RunState) => {
			if (next === state) return;
			state = next;
			node.dataset['run'] = next;
			onChange?.(next);
		};
		// Close to the viewport: hold the first frame, so nothing jumps when it scrolls in.
		const near = new IntersectionObserver(
			([entry]) => {
				if (entry?.isIntersecting && state === null) set('armed');
			},
			{ rootMargin: '30% 0px 30% 0px' }
		);
		const visible = new IntersectionObserver(
			([entry]) => {
				if (!entry) return;
				if (entry.intersectionRatio >= threshold) set('on');
				else if (!entry.isIntersecting && state === 'on') set('off');
			},
			{ threshold: [0, threshold] }
		);
		near.observe(node);
		visible.observe(node);
		return () => {
			near.disconnect();
			visible.disconnect();
		};
	};
}

/**
 * Pointer position as CSS variables on the element, for spotlights and tilt: `--mx`/`--my` in
 * pixels from its top left, `--px`/`--py` from -1 to 1 around its centre. Only for a fine pointer
 * that can hover, and never under reduced motion; written at most once a frame.
 */
export const pointerVars: Attachment<HTMLElement> = (node) => {
	if (reducedMotion() || !window.matchMedia('(hover: hover) and (pointer: fine)').matches) return;
	let frame = 0;
	let last: PointerEvent | null = null;
	const apply = () => {
		frame = 0;
		if (!last) return;
		const box = node.getBoundingClientRect();
		const x = last.clientX - box.left;
		const y = last.clientY - box.top;
		node.style.setProperty('--mx', `${x.toFixed(1)}px`);
		node.style.setProperty('--my', `${y.toFixed(1)}px`);
		node.style.setProperty('--px', ((x / box.width) * 2 - 1).toFixed(3));
		node.style.setProperty('--py', ((y / box.height) * 2 - 1).toFixed(3));
	};
	const move = (event: PointerEvent) => {
		last = event;
		node.dataset['pointer'] = 'in';
		if (!frame) frame = requestAnimationFrame(apply);
	};
	const leave = () => {
		last = null;
		delete node.dataset['pointer'];
		node.style.setProperty('--px', '0');
		node.style.setProperty('--py', '0');
	};
	node.addEventListener('pointermove', move);
	node.addEventListener('pointerleave', leave);
	return () => {
		cancelAnimationFrame(frame);
		node.removeEventListener('pointermove', move);
		node.removeEventListener('pointerleave', leave);
	};
};

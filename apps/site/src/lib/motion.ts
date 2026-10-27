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
 * A section's entrance (the reference's: it rises 64px and fades in, once, on a spring; app.css).
 * Only an element that starts below the fold is armed (`data-reveal="armed"`, invisible); it is
 * shown when a tenth of it is in view. Anything on screen at load is left alone, so nothing ever
 * blinks out; under reduced motion or without IntersectionObserver nothing is armed at all.
 */
export const reveal: Attachment<HTMLElement> = (node) => {
	if (reducedMotion() || !('IntersectionObserver' in window)) return;
	if (node.getBoundingClientRect().top < window.innerHeight) return;
	node.dataset['reveal'] = 'armed';
	const observer = new IntersectionObserver(
		([entry]) => {
			if (!entry?.isIntersecting) return;
			node.dataset['reveal'] = 'shown';
			observer.disconnect();
		},
		{ threshold: 0.1 }
	);
	observer.observe(node);
	return () => {
		observer.disconnect();
		delete node.dataset['reveal'];
	};
};

/**
 * Scroll-linked progress, from 0 when the element's top reaches `start` (a fraction of the
 * viewport's height from its top; 1 is the bottom edge) to 1 after `distance` more pixels of
 * scrolling (the element's own height when omitted). `apply` gets the progress at most once a
 * frame, and only while the element is near the screen. Under reduced motion it is never called,
 * so the element keeps its resting (final) frame. The measured box must not itself be transformed:
 * attach this to a wrapper and move a child.
 */
export function scrollLinked(
	apply: (progress: number, node: HTMLElement) => void,
	options: { start?: number; distance?: number; offset?: number } = {}
): Attachment<HTMLElement> {
	return (node) => {
		if (reducedMotion() || !('IntersectionObserver' in window)) return;
		const { start = 1, distance, offset = 0 } = options;
		let frame = 0;
		let near = false;
		const update = () => {
			frame = 0;
			const box = node.getBoundingClientRect();
			const span = distance ?? box.height;
			const progress = (window.innerHeight * start + offset - box.top) / span;
			apply(Math.min(1, Math.max(0, progress)), node);
		};
		const schedule = () => {
			if (near && !frame) frame = requestAnimationFrame(update);
		};
		const observer = new IntersectionObserver(
			([entry]) => {
				near = Boolean(entry?.isIntersecting);
				schedule();
			},
			{ rootMargin: '25% 0px 25% 0px' }
		);
		observer.observe(node);
		update();
		window.addEventListener('scroll', schedule, { passive: true });
		window.addEventListener('resize', schedule);
		return () => {
			cancelAnimationFrame(frame);
			observer.disconnect();
			window.removeEventListener('scroll', schedule);
			window.removeEventListener('resize', schedule);
		};
	};
}

/** The reference's ease for its load-time movements (an ease-out quart). */
export const easeOutQuart = (t: number) => 1 - (1 - Math.min(1, Math.max(0, t))) ** 4;

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

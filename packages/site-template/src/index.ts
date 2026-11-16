// The website starter bundle, as Colophon schema definitions (W.0): a `pages` collection whose
// `sections` field is the page builder, the shared `section-style` component every kind carries
// for per-section look overrides, and the `navigation` and `theme` single types. Installing it
// is ordinary API calls (a component, three content types, three seed entries); the bundle is
// data so the Studio, the testbed and the load scenario share one source.
//
// The field shapes mirror the server's wire format (docs/modules/01-domain.md; the Studio's
// FieldDefinition). Loose `Field` typing keeps the manifest readable; the package's own tests
// check the structural rules (depth, ids, kind count) and the install path is proven against
// the real server.

export interface Field {
	id: string;
	label: string;
	required: boolean;
	description?: string;
	default?: unknown;
	type: string;
	[option: string]: unknown;
}

export interface BlockKindDef {
	name: string;
	label: string;
	fields: Field[];
}

export interface TypeDef {
	slug: string;
	definition: {
		name: string;
		title_field: string;
		single?: boolean;
		fields: Field[];
	};
}

export interface ComponentDef {
	slug: string;
	definition: { name: string; fields: Field[] };
}

const text = (id: string, label: string, required = false, max_chars = 200, extra: Record<string, unknown> = {}): Field => ({
	id,
	label,
	required,
	type: 'text',
	max_chars,
	multiline: false,
	...extra
});

const multiline = (id: string, label: string, required = false, max_chars = 1000): Field => ({ id, label, required, type: 'text', max_chars, multiline: true });

const color = (id: string, label: string, fallback?: string): Field => ({
	id,
	label,
	required: fallback === undefined,
	type: 'color',
	...(fallback === undefined ? {} : { default: fallback })
});

const choice = (id: string, label: string, values: string[], required = false, fallback?: string): Field => ({
	id,
	label,
	required,
	type: 'enum',
	values,
	...(fallback === undefined ? {} : { default: fallback })
});

const integer = (id: string, label: string, min: number, max: number, required = true): Field => ({
	id,
	label,
	required,
	type: 'number',
	integer: true,
	min,
	max
});

const STYLE_FIELDS: Field[] = [
	choice('scheme', 'Colour treatment', ['inherit', 'surface', 'primary', 'secondary', 'invert']),
	choice('spacing', 'Space above and below', ['compact', 'comfortable', 'spacious']),
	choice('align', 'Text direction', ['start', 'center', 'end'])
];

/** The look options every section kind offers; blank means the theme decides. */
export const SECTION_STYLE_COMPONENT: ComponentDef = {
	slug: 'section-style',
	definition: {
		name: 'Section style',
		fields: STYLE_FIELDS
	}
};

const style = (): Field => ({
	id: 'style',
	label: 'Style',
	required: false,
	description: 'How this section looks. Anything left unset follows the theme.',
	type: 'object',
	component: 'section-style',
	fields: STYLE_FIELDS
});

/** The section kinds a page composes (the builder's fixed layouts). Six, of twenty allowed. */
export const SECTION_KINDS: BlockKindDef[] = [
	{
		name: 'hero',
		label: 'Hero',
		fields: [
			text('heading', 'Heading', true, 120),
			multiline('tagline', 'Tagline', false, 280),
			{ id: 'image', label: 'Image', required: false, type: 'asset', many: false },
			text('alt', 'Image description', false, 200),
			text('label', 'Button label', false, 40),
			{ id: 'url', label: 'Button link', required: false, type: 'url' },
			style()
		]
	},
	{
		name: 'rich_text',
		label: 'Rich text',
		fields: [{ id: 'content', label: 'Content', required: true, type: 'rich_text' }, style()]
	},
	{
		name: 'features',
		label: 'Feature list',
		fields: [
			text('heading', 'Heading', false, 120),
			{
				id: 'items',
				label: 'Features',
				required: false,
				type: 'list',
				max: 12,
				of: {
					type: 'object',
					fields: [text('title', 'Title', true, 80), text('text', 'Text', false, 200)]
				}
			},
			style()
		]
	},
	{
		name: 'testimonial',
		label: 'Testimonial',
		fields: [multiline('quote', 'Quote', true, 400), text('author', 'Author', false, 80), text('role', 'Role', false, 80), style()]
	},
	{
		name: 'cta',
		label: 'Call to action',
		fields: [text('heading', 'Heading', true, 120), multiline('text', 'Text', false, 280), text('label', 'Button label', true, 40), { id: 'url', label: 'Button link', required: true, type: 'url' }, style()]
	},
	{
		name: 'faq',
		label: 'Questions and answers',
		fields: [
			text('heading', 'Heading', false, 120),
			{
				id: 'items',
				label: 'Questions',
				required: false,
				type: 'list',
				max: 20,
				of: {
					type: 'object',
					fields: [text('question', 'Question', true, 200), { id: 'answer', label: 'Answer', required: true, type: 'markdown', max_chars: 1000 }]
				}
			},
			style()
		]
	}
];

export const PAGES_TYPE: TypeDef = {
	slug: 'pages',
	definition: {
		name: 'Pages',
		title_field: 'title',
		fields: [
			text('title', 'Title', true, 200),
			{ id: 'slug', label: 'Slug', required: true, type: 'slug', source: 'title', description: 'The page’s address, like `about-us`.' },
			{
				id: 'seo',
				label: 'Search listing',
				required: false,
				description: 'What search engines show for this page.',
				type: 'object',
				fields: [multiline('description', 'Description', false, 160)]
			},
			{
				id: 'sections',
				label: 'Sections',
				required: false,
				description: 'The page, top to bottom. Add, arrange and style sections here or in the visual builder.',
				type: 'blocks',
				max: 24,
				blocks: SECTION_KINDS
			}
		]
	}
};

const NAV_ITEM: Field = {
	id: 'items',
	label: 'Menu items',
	required: false,
	description: 'Each links to a page of this site, or to an outside address.',
	type: 'list',
	max: 20,
	of: {
		type: 'object',
		fields: [
			text('label', 'Label', true, 40),
			{ id: 'page', label: 'Page', required: false, type: 'reference', to: 'pages' },
			{ id: 'url', label: 'Outside link', required: false, type: 'url' }
		]
	}
};

export const NAVIGATION_TYPE: TypeDef = {
	slug: 'navigation',
	definition: {
		name: 'Navigation',
		title_field: 'title',
		single: true,
		fields: [{ ...text('title', 'Title', true, 60), default: 'Navigation' }, NAV_ITEM]
	}
};

const FONTS = ['system', 'sans', 'serif', 'mono'];

/** The design system's editable part: the whole token set for one mode. Required, without
 *  defaults — only top-level fields may default, so the seed writes the palette in full. */
const MODE_FIELDS: Field[] = [
	color('primary', 'Primary'),
	color('secondary', 'Secondary'),
	color('accent', 'Accent'),
	color('surface', 'Surface'),
	color('text', 'Text'),
	color('muted', 'Muted text'),
	integer('radius_sm', 'Small corners (px)', 0, 24),
	integer('radius_md', 'Medium corners (px)', 0, 32),
	integer('radius_lg', 'Large corners (px)', 0, 48),
	choice('shadow', 'Shadows', ['none', 'subtle', 'raised', 'floating'], true),
	choice('font_body', 'Body font', FONTS, true),
	choice('font_heading', 'Heading font', FONTS, true),
	integer('base_size', 'Base text size (px)', 14, 20)
];

/** The palette the starter theme holds (and the renderer falls back to before one is saved). */
export const DEFAULT_MODE: Record<string, unknown> = {
	primary: '#6d28d9',
	secondary: '#0f766e',
	accent: '#d97706',
	surface: '#ffffff',
	text: '#18181b',
	muted: '#71717a',
	radius_sm: 4,
	radius_md: 8,
	radius_lg: 16,
	shadow: 'subtle',
	font_body: 'system',
	font_heading: 'system',
	base_size: 16
};

export const THEME_TYPE: TypeDef = {
	slug: 'theme',
	definition: {
		name: 'Theme',
		title_field: 'title',
		single: true,
		fields: [
			{ ...text('title', 'Title', true, 60), default: 'Site theme' },
			{
				id: 'light',
				label: 'Light mode',
				required: true,
				description: 'The design system visitors see: colours, corners, shadows and type.',
				type: 'object',
				fields: MODE_FIELDS
			},
			{
				id: 'dark',
				label: 'Dark mode',
				required: false,
				description: 'An optional second palette for visitors who prefer dark.',
				type: 'object',
				fields: MODE_FIELDS
			},
			{
				id: 'tokens',
				label: 'Custom tokens',
				required: false,
				description: 'Your own design tokens, published as --site-x-<name> CSS variables.',
				type: 'list',
				max: 50,
				of: {
					type: 'object',
					fields: [
						{ ...text('name', 'Name', true, 32), pattern: '^[a-z][a-z0-9_]{0,31}$' },
						text('value', 'Value', true, 200)
					]
				}
			}
		]
	}
};

/** The bundle's install order: the component first (types echo it), then the types, then seeds. */
export const SITE_COMPONENTS: ComponentDef[] = [SECTION_STYLE_COMPONENT];
export const SITE_TYPES: TypeDef[] = [PAGES_TYPE, NAVIGATION_TYPE, THEME_TYPE];

/** The starter entries, in creation order: the home page first (the navigation links to it),
 *  then navigation, then the theme — the theme's defaults come from the type. */
export interface SeedEntry {
	collection: string;
	content: Record<string, unknown>;
}

/** The starter home page: a hero and a call to action, in canonical `_block`-first form. */
export const HOME_PAGE: Record<string, unknown> = {
	title: 'Home',
	slug: 'home',
	sections: [
		{
			_block: 'hero',
			heading: 'Your site, your way',
			tagline: 'Sections you arrange, words you change, a design system that is yours.',
			label: 'Start writing',
			url: 'https://example.com/'
		},
		{
			_block: 'cta',
			heading: 'Ready when you are',
			text: 'Change these words, or take the section out: every part of this page is content.',
			label: 'Read more',
			url: 'https://example.com/'
		}
	]
};

/** The navigation entry once the home page exists (`home` is the created entry's id). */
export const navigationSeed = (home: string): Record<string, unknown> => ({ title: 'Navigation', items: [{ label: 'Home', page: home }] });

/** The theme entry: the starter palette in canonical definition order. */
export const THEME_SEED: Record<string, unknown> = { title: 'Site theme', light: DEFAULT_MODE };

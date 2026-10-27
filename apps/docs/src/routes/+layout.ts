// Every page is prerendered to a file (module 16). Addresses end in a slash, so each page is a
// folder with an index.html that any static host serves as it is.
export const prerender = true;
export const trailingSlash = 'always';

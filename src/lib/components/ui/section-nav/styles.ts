/**
 * Item shape shared by `section-nav` and `nav-list`, so the Manage section list and the app
 * sidebar read as one family. Each list adds its own padding, alignment and active selector.
 */
export const navItemBase =
	"flex h-9 items-center gap-2 rounded-md border-2 border-transparent text-sm font-medium transition-all focus:outline-hidden focus-visible:shadow-xs disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0";

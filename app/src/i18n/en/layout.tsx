// The app's frame: the top navigation, the Ctrl+K palette, the ask bar and the shell
// (components/layout). The navigation's names are in `common.nav`.
export const layout = {
  topNav: {
    search: 'Search…',
    lightTheme: 'Light theme',
    darkTheme: 'Dark theme',
    /** The avatar when no name is set. */
    me: 'Me',
  },
  askBar: {
    placeholder: 'Ask anything… search your knowledge, find information, or get help with your tasks.',
    ask: 'Ask',
  },
  palette: {
    description: 'Search tasks and knowledge',
    placeholder: 'Search tasks, pages, or type a command…',
    noResults: 'No results.',
    actions: 'Actions',
    goTo: 'Go to',
    newPage: 'New knowledge page',
    askAssistant: 'Ask the assistant',
    /** A result found only by its meaning. */
    byMeaning: 'by meaning',
    searchingByMeaning: 'Searching by meaning…',
  },
  shell: {
    /** Only in a plain browser tab, never in the app. */
    browserPreview: (
      <>
        Browser preview: data from the Rust core is not available here. Run the app with <code>cargo tauri dev</code>.
      </>
    ),
  },
}

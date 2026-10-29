import { createBrowserRouter, Navigate, RouterProvider } from 'react-router'
import { AppShell } from '@/components/layout/AppShell'
import AssistantPage from '@/features/assistant/AssistantPage'
import MergePage from '@/features/creation/MergePage'
import NewPagePage from '@/features/creation/NewPagePage'
import GlossaryPage from '@/features/glossary/GlossaryPage'
import HomePage from '@/features/home/HomePage'
import KnowledgePage from '@/features/knowledge/KnowledgePage'
import CategoryPage from '@/features/categories/CategoryPage'
import CategoriesPage from '@/features/categories/CategoriesPage'
import SettingsPage from '@/features/settings/SettingsPage'
import TaskDetailPage from '@/features/tasks/TaskDetailPage'
import TasksPage from '@/features/tasks/TasksPage'

// Route map. Feature folders own their pages; see app/README.md.
const router = createBrowserRouter([
  {
    element: <AppShell />,
    children: [
      { path: '/', element: <HomePage /> },
      { path: '/tasks', element: <TasksPage /> },
      { path: '/tasks/:taskKey', element: <TaskDetailPage /> },
      { path: '/knowledge', element: <KnowledgePage /> },
      { path: '/knowledge/new', element: <NewPagePage /> },
      { path: '/knowledge/merge/:targetPageId', element: <MergePage /> },
      // The glossary moved to its own tab; the old address (before :pageId) still leads there.
      { path: '/knowledge/glossary', element: <Navigate to="/glossary" replace /> },
      { path: '/knowledge/:pageId', element: <KnowledgePage /> },
      { path: '/categories', element: <CategoriesPage /> },
      { path: '/glossary', element: <GlossaryPage /> },
      { path: '/categories/:categoryId', element: <CategoryPage /> },
      { path: '/assistant', element: <AssistantPage /> },
      { path: '/assistant/:conversationId', element: <AssistantPage /> },
      { path: '/settings', element: <SettingsPage /> },
      { path: '/settings/:tab', element: <SettingsPage /> },
    ],
  },
])

export default function App() {
  return <RouterProvider router={router} />
}

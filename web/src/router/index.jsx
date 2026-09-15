import {createBrowserRouter, Navigate} from "react-router-dom";
import App from "@/pages/App.jsx";
import Home from "@/pages/Home/index.jsx";
import NotFound from "@/pages/NotFound/index.jsx";
import Dashboard from "@/pages/Dashboard/index.jsx";
import Log from "@/pages/Dashboard/Log/index.jsx";
import Help from "@/pages/Dashboard/Help/index.jsx";
import Settings from "@/pages/Dashboard/Settings/index.jsx";
import Login from "@/pages/Login/index.jsx";
import Personalization from "@/pages/Dashboard/Personalization/index.jsx";

const router = createBrowserRouter([
  {
    path: '/',
    element: <App/>,
    children: [
      {
        index: true,
        element: <Home/>
      },
      {
        path: 'login',
        element: <Login/>
      },
      {
        path: 'dashboard',
        element: <Dashboard/>,
        children: [
          {
            index: true,
            element: <Log/>
          },
          {
            path: 'directory',
            element: <Navigate to="/dashboard" replace/>
          },
          {
            path: 'log',
            element: <Navigate to="/dashboard" replace/>
          },
          {
            path: 'help',
            element: <Help/>
          },
          {
            path: 'settings',
            element: <Settings/>
          },
          {
            path: 'personalization',
            element: <Personalization/>
          }
        ]
      },
      {
        path: '*',
        element: <NotFound/>
      }
    ]
  }
])

export default router

import {Outlet, useLocation, useNavigate} from "react-router-dom";
import {AppWindow, CircleHelp, CircleUserRound, LogOut, Settings} from "lucide-react";
import {userCheckTokenRequest, userSignOutRequest} from "@/api/user.js";
import {message} from "antd";
import {useDispatch, useSelector} from "react-redux";
import {clearToken} from "@/store/modules/userStore.js";
import {useEffect} from "react";

const navs = [
  {
    nav: '/dashboard',
    name: '工作台',
    icon: <AppWindow size={16} strokeWidth={1.5}/>
  }
]

const navsFooter = [
  {
    nav: '/dashboard/help',
    name: '帮助',
    icon: <CircleHelp size={16} strokeWidth={1.5}/>
  },
  {
    nav: '/dashboard/settings',
    name: '设置',
    icon: <Settings size={16} strokeWidth={1.5}/>
  }
]

const Index = () => {

  const navigate = useNavigate();
  const location = useLocation();
  const user = useSelector((state) => state.user);
  const dispatch = useDispatch();
  const [messageApi, contextHolder] = message.useMessage();

  useEffect(() => {
    checkToken()
  }, []);

  const checkToken = async () => {
    if (!user.token) {
      navigate("/login?type=notLogin");
    }

    const {code, msg, data} = await userCheckTokenRequest();
    if (code !== 1) {
      dispatch(clearToken())
      navigate('/login?type=checkToken');
    }
  }

  const signOut = async () => {
    const {code, msg, data} = await userSignOutRequest()
    if (code === 1) {
      dispatch(clearToken())
      navigate('/login?type=signOut');
    } else {
      messageApi.error(msg)
    }
  }

  return (
    <>
      {contextHolder}
      <div className="flex">
        <div
          className="fixed top-0 left-0 h-full w-full space-y-8 border-r border-[#dce7e4] bg-[#fbfdfc] dark:border-[#263532] dark:bg-[#101615] sm:w-60">
          <div className="flex flex-col h-full">
            <div className='h-20 flex justify-center items-center px-8'>
              <div className='flex-none cursor-pointer' onClick={() => navigate('/')}>
                <div className="text-3xl font-bold text-teal-700 dark:text-teal-400">MCUpdate</div>
              </div>
            </div>
            <div className="flex-1 flex flex-col h-full overflow-auto">
              <ul className="px-4 text-sm font-medium flex-1">
                {
                  navs.map((item, idx) => {
                    const isActive = location.pathname === item.nav
                    return (
                      <li key={idx}>
                        <div onClick={() => navigate(item.nav)}
                             className={`flex cursor-pointer items-center gap-x-2 rounded-md p-2 ${isActive ? 'bg-teal-50 text-teal-800 dark:bg-teal-950/50 dark:text-teal-200' : 'text-gray-600 hover:bg-[#edf4f2] active:bg-[#e3efec] dark:text-gray-200 dark:hover:bg-[#192321] dark:active:bg-[#21302d]'} duration-150`}>
                          <div className={isActive ? 'text-teal-700 dark:text-teal-300' : 'text-gray-500 dark:text-gray-300'}>{item.icon}</div>
                          {item.name}
                        </div>
                      </li>
                    )
                  })
                }
              </ul>
              <div>
                <ul className="px-4 pb-4 text-sm font-medium">
                  {
                    navsFooter.map((item, idx) => {
                      const isActive = location.pathname === item.nav
                      return (
                        <li key={idx}>
                          <div onClick={() => navigate(item.nav)}
                               className={`flex cursor-pointer items-center gap-x-2 rounded-md p-2 ${isActive ? 'bg-teal-50 text-teal-800 dark:bg-teal-950/50 dark:text-teal-200' : 'text-gray-600 hover:bg-[#edf4f2] active:bg-[#e3efec] dark:text-gray-200 dark:hover:bg-[#192321] dark:active:bg-[#21302d]'} duration-150`}>
                            <div className={isActive ? 'text-teal-700 dark:text-teal-300' : 'text-gray-500 dark:text-gray-300'}>{item.icon}</div>
                            {item.name}
                          </div>
                        </li>
                      )
                    })
                  }
                  <li>
                    <div
                      onClick={() => signOut()}
                      className="flex cursor-pointer items-center gap-x-2 rounded-md p-2 text-gray-600 duration-150 hover:bg-[#edf4f2] active:bg-[#e3efec] dark:text-gray-200 dark:hover:bg-[#192321] dark:active:bg-[#21302d]">
                      <div className="text-gray-500 dark:text-white"><LogOut size={16} strokeWidth={1.5}/></div>
                      退出登录
                    </div>
                  </li>
                </ul>
                <div className="py-4 px-4 border-t dark:border-gray-900">
                  <div className="flex items-center gap-x-4">
                    {/*<img src="" className="w-12 h-12 rounded-full"/>*/}
                    <CircleUserRound className="dark:text-white" size={40} strokeWidth={1.0}/>
                    <div>
                      <span className="block text-gray-700 dark:text-white text-sm font-semibold">ADMIN</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div className="ml-60 flex-grow">
          <Outlet/>
        </div>
      </div>
    </>
  );
};

export default Index;

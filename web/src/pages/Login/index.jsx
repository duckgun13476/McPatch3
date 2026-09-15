import {useNavigate, useSearchParams} from "react-router-dom";
import {useDispatch, useSelector} from "react-redux";
import {userLogin} from "@/store/modules/userStore.js";
import {message} from "antd";
import {useEffect, useState} from "react";

const Index = () => {

  const [username, setUsername] = useState('')
  const navigate = useNavigate()
  const [params] = useSearchParams();
  const dispatch = useDispatch()
  const [messageApi, contextHolder] = message.useMessage();
  const user = useSelector(state => state.user)

  useEffect(() => {
    if (user.username) {
      setUsername(user.username);
    }

    const type = params.get('type');
    switch (type) {
      case 'signOut':
        messageApi.success('退出成功!');
        break;
      case 'changeUsername':
        messageApi.success('修改用户名成功!');
        break;
      case 'changePassword':
        messageApi.success('修改密码成功!');
        break;
      case 'notLogin':
        messageApi.success('请先登录!');
        break;
      case 'checkToken':
        messageApi.success('Token无效!');
        break;
    }
  }, []);

  const login = async (e) => {
    e.preventDefault()

    const password = e.target[1].value

    const {flag, msg} = await dispatch(userLogin(username, password));
    if (flag) {
      navigate('/dashboard');
    } else {
      messageApi.error(msg);
    }
  }

  return (
    <>
      {contextHolder}
      <div className="w-full h-screen flex flex-col items-center justify-center px-4">
        <div className="max-w-sm w-full text-gray-500 dark:text-white space-y-5">
          <div className="text-center pb-8">
            <div className="text-4xl font-bold text-teal-700 dark:text-teal-400">MCUpdate</div>
          </div>
          <form
            onSubmit={login}
            className="space-y-5">
            <div>
              <label>
                用户名
              </label>
              <input
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                name="username"
                type="text"
                required
                className="mt-2 w-full rounded-md border bg-transparent px-3 py-2 shadow-sm outline-none focus:border-teal-600"
              />
            </div>
            <div>
              <label>
                密码
              </label>
              <input
                name="password"
                type="password"
                required
                className="mt-2 w-full rounded-md border bg-transparent px-3 py-2 shadow-sm outline-none focus:border-teal-600"
              />
            </div>
            <div className="flex items-center justify-between text-sm">
              <a href="#"
                 className="text-center text-teal-700 hover:text-teal-600 dark:text-teal-400 dark:hover:text-teal-300">忘记密码?</a>
            </div>
            <button
              className="w-full rounded-md bg-teal-700 px-4 py-2 font-medium text-white duration-150 hover:bg-teal-600 active:bg-teal-800"
              type="submit">
              登录
            </button>
          </form>
        </div>
      </div>
    </>
  );
};

export default Index;

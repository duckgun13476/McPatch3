import {useNavigate} from "react-router-dom";
import {useSelector} from "react-redux";
import {theme} from "antd";

const {useToken} = theme;

const Index = () => {

  const user = useSelector(state => state.user)
  const navigate = useNavigate();
  const {token} = useToken();

  const checkStatus = () => {
    if (user.token) {
      navigate("/dashboard")
    } else {
      navigate("/login")
    }
  }

  return (
    <>
      <div className="w-screen h-screen flex flex-col justify-center items-center space-y-4">
        <h2
          className="text-transparent bg-clip-text bg-gradient-to-r from-[#4F46E5] to-[#E114E5] text-4xl font-extrabold md:text-5xl">
          MCUpdate
        </h2>
        <p className={`max-w-2xl mx-auto text-center dark:text-white`}>
          MCUpdate 是一个面向 Minecraft 客户端的独立文件更新程序。你可以通过它向服务器玩家安全地分发整合包内容。
        </p>
        <button
          onClick={() => checkStatus()}
          className="px-6 py-3.5 text-white bg-indigo-600 rounded-full duration-150 hover:bg-indigo-500 active:bg-indigo-700">
          即刻开始!
        </button>
      </div>
    </>
  );
};

export default Index;

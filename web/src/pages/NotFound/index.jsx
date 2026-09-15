import {ArrowBigLeftDash} from "lucide-react";
import {useNavigate} from "react-router-dom";

const Index = () => {
  const navigate = useNavigate();

  return (
    <>
      <div className="max-w-screen-xl mx-auto px-4 flex items-center justify-start h-screen md:px-8">
        <div className="max-w-lg mx-auto space-y-3 text-center">
          <div className="text-4xl text-teal-700 dark:text-teal-400">MCUpdate</div>
          <p className="text-gray-600 dark:text-gray-50">
            404. 对不起,您要查找的页面无法找到或已被删除.
          </p>
          <div onClick={() => navigate(-1)}
               className="inline-flex cursor-pointer items-center gap-x-1 font-medium text-teal-700 duration-150 hover:text-teal-500">
            返回
            <ArrowBigLeftDash size={16} strokeWidth={1.5}/>
          </div>
        </div>
      </div>
    </>
  );
};

export default Index;

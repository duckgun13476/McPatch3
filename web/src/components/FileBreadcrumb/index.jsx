import React from 'react';
import {Tooltip} from 'antd';

const Index = ({path, handlerBreadcrumb, workspacePath}) => {

  const items = ['/', ...path]
  return (
    <>
      <div className="h-16 border-l-2 border-teal-600 pr-4 pl-4">
        <Tooltip title={workspacePath || '工作空间根目录'} placement="topLeft">
          <div className="flex h-8 w-fit cursor-help items-center text-base font-bold text-teal-700 dark:text-teal-400">工作目录</div>
        </Tooltip>
        <ul className="h-8 flex items-center">
          {
            items.map((item, index) => {
              return (
                <li key={index} className="flex items-center cursor-default">
                  {
                    items.length - 1 !== index ?
                      <div className="text-base text-gray-800 dark:text-gray-500 font-medium">
                        <button onClick={() => handlerBreadcrumb(index)}>{item}</button>
                        <span className="px-3 text-body-color">{" / "}</span>
                      </div> :
                      <div className="text-base text-gray-400 dark:text-white font-medium">
                        {item}
                      </div>
                  }
                </li>
              )
            })
          }
        </ul>
      </div>
    </>
  );
};

export default Index;

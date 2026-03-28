import React, { useState, useEffect } from 'react';

const TaskDispatcher = () => {
  const [taskContent, setTaskContent] = useState('');
  const [taskType, setTaskType] = useState('code_generation');
  const [priority, setPriority] = useState('normal');
  const [targetPlatform, setTargetPlatform] = useState('');
  const [timeout, setTimeout] = useState(30);
  const [taskId, setTaskId] = useState(null);
  const [taskStatus, setTaskStatus] = useState(null);
  const [queueStatus, setQueueStatus] = useState(null);
  const [isLoading, setIsLoading] = useState(false);

  // 获取队列状态
  const fetchQueueStatus = async () => {
    try {
      const response = await window.__TAURI__.invoke('get_queue_status');
      setQueueStatus(response);
    } catch (error) {
      console.error('获取队列状态失败:', error);
    }
  };

  // 页面加载时获取队列状态
  useEffect(() => {
    fetchQueueStatus();
    // 每5秒刷新一次队列状态
    const interval = setInterval(fetchQueueStatus, 5000);
    return () => clearInterval(interval);
  }, []);

  // 提交任务
  const handleSubmitTask = async (e) => {
    e.preventDefault();
    setIsLoading(true);

    try {
      const request = {
        task_type: taskType,
        priority,
        content: taskContent,
        target_platform: targetPlatform || null,
        timeout_seconds: timeout,
      };

      const response = await window.__TAURI__.invoke('submit_task', { request });
      setTaskId(response.task_id);
      setTaskStatus(null);

      // 获取任务状态
      const statusResponse = await window.__TAURI__.invoke('get_task_status', { task_id: response.task_id });
      setTaskStatus(statusResponse);

      // 刷新队列状态
      fetchQueueStatus();

    } catch (error) {
      console.error('提交任务失败:', error);
      alert(`提交任务失败: ${error.message}`);
    } finally {
      setIsLoading(false);
    }
  };

  // 获取任务状态
  const handleCheckStatus = async () => {
    if (!taskId) {
      alert('请输入任务ID');
      return;
    }

    try {
      const response = await window.__TAURI__.invoke('get_task_status', { task_id: taskId });
      setTaskStatus(response);
    } catch (error) {
      console.error('获取任务状态失败:', error);
      alert(`获取任务状态失败: ${error.message}`);
    }
  };

  // 手动执行下一个任务（主要用于演示）
  const handleExecuteNext = async () => {
    try {
      const response = await window.__TAURI__.invoke('execute_next_task');
      alert(`执行下一个任务: ${response ? response : '无任务'}`);
      fetchQueueStatus(); // 刷新队列状态
    } catch (error) {
      console.error('执行任务失败:', error);
      alert(`执行任务失败: ${error.message}`);
    }
  };

  return (
    <div className="container mx-auto px-4 py-8">
      <h1 className="text-3xl font-bold mb-6">AI任务分派系统</h1>

      {/* 任务提交表单 */}
      <div className="bg-white rounded-lg shadow-md p-6 mb-6">
        <h2 className="text-xl font-semibold mb-4">提交新任务</h2>

        <form onSubmit={handleSubmitTask}>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                任务类型
              </label>
              <select
                value={taskType}
                onChange={(e) => setTaskType(e.target.value)}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                required
              >
                <option value="code_generation">代码生成</option>
                <option value="text_analysis">文本分析</option>
                <option value="math_calculation">数学计算</option>
                <option value="creative_writing">创意写作</option>
                <option value="translation">翻译</option>
                <option value="custom:general">自定义任务</option>
              </select>
            </div>

            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                优先级
              </label>
              <select
                value={priority}
                onChange={(e) => setPriority(e.target.value)}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                required
              >
                <option value="low">低</option>
                <option value="normal">普通</option>
                <option value="high">高</option>
                <option value="critical">紧急</option>
              </select>
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                目标平台 (可选)
              </label>
              <select
                value={targetPlatform}
                onChange={(e) => setTargetPlatform(e.target.value)}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
              >
                <option value="">自动选择</option>
                <option value="claude">Claude</option>
                <option value="codex">Codex</option>
                <option value="gemini">Gemini</option>
                <option value="opencode">OpenCode</option>
              </select>
            </div>

            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                超时时间 (秒)
              </label>
              <input
                type="number"
                value={timeout}
                onChange={(e) => setTimeout(Number(e.target.value))}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                min="1"
                required
              />
            </div>
          </div>

          <div className="mb-4">
            <label className="block text-sm font-medium text-gray-700 mb-1">
              任务内容
            </label>
            <textarea
              value={taskContent}
              onChange={(e) => setTaskContent(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
              rows="4"
              placeholder="输入您的任务内容..."
              required
            />
          </div>

          <button
            type="submit"
            disabled={isLoading}
            className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isLoading ? '提交中...' : '提交任务'}
          </button>
        </form>
      </div>

      {/* 队列状态 */}
      {queueStatus && (
        <div className="bg-white rounded-lg shadow-md p-6 mb-6">
          <h2 className="text-xl font-semibold mb-4">队列状态</h2>
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="bg-blue-50 p-4 rounded-lg">
              <div className="text-sm text-blue-600">待处理任务</div>
              <div className="text-2xl font-bold text-blue-800">{queueStatus.pending_tasks}</div>
            </div>
            <div className="bg-green-50 p-4 rounded-lg">
              <div className="text-sm text-green-600">活跃任务</div>
              <div className="text-2xl font-bold text-green-800">{queueStatus.active_tasks}</div>
            </div>
            <div className="bg-purple-50 p-4 rounded-lg">
              <div className="text-sm text-purple-600">历史任务</div>
              <div className="text-2xl font-bold text-purple-800">{queueStatus.history_count}</div>
            </div>
          </div>
        </div>
      )}

      {/* 任务结果展示 */}
      {(taskId || taskStatus) && (
        <div className="bg-white rounded-lg shadow-md p-6 mb-6">
          <h2 className="text-xl font-semibold mb-4">任务状态</h2>

          <div className="mb-4">
            <label className="block text-sm font-medium text-gray-700 mb-1">
              任务ID
            </label>
            <div className="flex">
              <input
                type="text"
                value={taskId || ''}
                onChange={(e) => setTaskId(e.target.value)}
                className="flex-1 px-3 py-2 border border-gray-300 rounded-l-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                placeholder="输入任务ID查询状态"
              />
              <button
                onClick={handleCheckStatus}
                className="px-4 py-2 bg-green-600 text-white rounded-r-md hover:bg-green-700"
              >
                查询状态
              </button>
            </div>
          </div>

          {taskStatus && (
            <div className="mt-4 p-4 bg-gray-50 rounded-lg">
              <div className="grid grid-cols-1 md:grid-cols-2 gap-2 mb-2">
                <div><strong>任务ID:</strong> {taskStatus.task_id}</div>
                <div><strong>状态:</strong>
                  <span className={`ml-2 px-2 py-1 rounded text-xs ${
                    taskStatus.status === 'completed' ? 'bg-green-200 text-green-800' :
                    taskStatus.status === 'pending' ? 'bg-yellow-200 text-yellow-800' :
                    taskStatus.status === 'running' ? 'bg-blue-200 text-blue-800' :
                    taskStatus.status === 'failed' ? 'bg-red-200 text-red-800' :
                    taskStatus.status === 'timeout' ? 'bg-orange-200 text-orange-800' :
                    'bg-gray-200 text-gray-800'
                  }`}>
                    {taskStatus.status}
                  </span>
                </div>
                {taskStatus.executed_on && (
                  <div><strong>执行平台:</strong> {taskStatus.executed_on}</div>
                )}
                {taskStatus.completed_at && (
                  <div><strong>完成时间:</strong> {new Date(taskStatus.completed_at * 1000).toLocaleString()}</div>
                )}
              </div>

              {taskStatus.result && (
                <div className="mt-2">
                  <strong>结果:</strong>
                  <div className="mt-1 p-2 bg-white border rounded text-sm whitespace-pre-wrap">
                    {taskStatus.result}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>
      )}

      {/* 管理按钮（用于演示） */}
      <div className="bg-white rounded-lg shadow-md p-6">
        <h2 className="text-xl font-semibold mb-4">管理操作</h2>
        <div className="flex flex-wrap gap-2">
          <button
            onClick={fetchQueueStatus}
            className="px-4 py-2 bg-gray-600 text-white rounded-md hover:bg-gray-700"
          >
            刷新队列状态
          </button>
          <button
            onClick={handleExecuteNext}
            className="px-4 py-2 bg-indigo-600 text-white rounded-md hover:bg-indigo-700"
          >
            执行下一个任务
          </button>
        </div>
      </div>
    </div>
  );
};

export default TaskDispatcher;
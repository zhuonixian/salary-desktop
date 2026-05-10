import { useState } from 'react';
import { HashRouter, Routes, Route, useNavigate, useLocation } from 'react-router-dom';
import { Layout, Menu, DatePicker } from 'antd';
import {
  DashboardOutlined,
  TeamOutlined,
  CalendarOutlined,
  ScanOutlined,
  SettingOutlined,
  CalculatorOutlined,
  ExportOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  FormOutlined,
} from '@ant-design/icons';
import dayjs from 'dayjs';
import type { Dayjs } from 'dayjs';

import Dashboard from '@/pages/Dashboard';
import Employees from '@/pages/Employees';
import Attendance from '@/pages/Attendance';
import OcrCenter from '@/pages/OcrCenter';
import SalaryRules from '@/pages/SalaryRules';
import SalaryCalculate from '@/pages/SalaryCalculate';
import ExportCenter from '@/pages/ExportCenter';
import PunchCard from '@/pages/PunchCard';

const { Sider, Header, Content } = Layout;

const menuItems = [
  { key: '/', label: '首页仪表盘', icon: <DashboardOutlined /> },
  { key: '/employees', label: '员工管理', icon: <TeamOutlined /> },
  { key: '/attendance', label: '考勤管理', icon: <CalendarOutlined /> },
  { key: '/punch-card', label: '打卡表管理', icon: <FormOutlined /> },
  { key: '/ocr', label: 'OCR识别中心', icon: <ScanOutlined /> },
  { key: '/rules', label: '规则配置', icon: <SettingOutlined /> },
  { key: '/salary', label: '工资计算', icon: <CalculatorOutlined /> },
  { key: '/export', label: '导出中心', icon: <ExportOutlined /> },
];

const AppLayout: React.FC = () => {
  const [collapsed, setCollapsed] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const [globalMonth, setGlobalMonth] = useState<Dayjs>(dayjs());

  return (
    <Layout className="app-layout">
      <Sider
        className="app-sider"
        trigger={null}
        collapsible
        collapsed={collapsed}
        width={220}
        theme="dark"
      >
        <div className={`logo ${collapsed ? 'collapsed' : ''}`}>
          {collapsed ? '工资' : '工资核算助手'}
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[location.pathname]}
          items={menuItems}
          onClick={({ key }) => navigate(key)}
          style={{ borderRight: 0 }}
        />
      </Sider>

      <div className={`app-content-wrapper ${collapsed ? 'collapsed' : ''}`}>
        <Header className="app-header">
          <div className="app-header-left">
            <span
              className="trigger-btn"
              onClick={() => setCollapsed(!collapsed)}
            >
              {collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
            </span>
          </div>
          <div className="app-header-right">
            <span style={{ color: '#666', fontSize: 14 }}>当前月份：</span>
            <DatePicker
              picker="month"
              value={globalMonth}
              onChange={(d) => d && setGlobalMonth(d)}
              allowClear={false}
              style={{ width: 160 }}
            />
          </div>
        </Header>

        <Content className="app-main">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/employees" element={<Employees />} />
            <Route path="/attendance" element={<Attendance />} />
            <Route path="/punch-card" element={<PunchCard />} />
            <Route path="/ocr" element={<OcrCenter />} />
            <Route path="/rules" element={<SalaryRules />} />
            <Route path="/salary" element={<SalaryCalculate />} />
            <Route path="/export" element={<ExportCenter />} />
          </Routes>
        </Content>
      </div>
    </Layout>
  );
};

function App() {
  return (
    <HashRouter>
      <AppLayout />
    </HashRouter>
  );
}

export default App;

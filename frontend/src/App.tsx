import { BrowserRouter, Routes, Route } from "react-router-dom";
import { Layout } from "@/components/Layout";
import Library from "@/pages/Library";
import Search from "@/pages/Search";
import QA from "@/pages/QA";
import Entities from "@/pages/Entities";
import Settings from "@/pages/Settings";

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<Layout />}>
          <Route index element={<Library />} />
          <Route path="search" element={<Search />} />
          <Route path="qa" element={<QA />} />
          <Route path="entities" element={<Entities />} />
          <Route path="settings" element={<Settings />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}

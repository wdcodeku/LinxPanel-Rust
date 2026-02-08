import axios from 'axios'
import { ElMessage } from 'element-plus'
import router from '@/router'

const request = axios.create({
  baseURL: '/api',
  timeout: 30000,
})

request.interceptors.request.use((config) => {
  const token = localStorage.getItem('token')
  if (token) config.headers.Authorization = `Bearer ${token}`
  return config
})

request.interceptors.response.use(
  (res) => {
    if (res.data && !res.data.success) {
      ElMessage.error(res.data.message || 'Request failed')
    }
    return res.data
  },
  (err) => {
    if (err.response?.status === 401) {
      localStorage.removeItem('token')
      router.push('/login')
    }
    ElMessage.error(err.message || 'Network error')
    return Promise.reject(err)
  },
)

export default request